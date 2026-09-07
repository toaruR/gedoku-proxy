//! 氏名検出 (L3' `ja_name_*`, 設計 §7.10.6)。
//!
//! `surname_dict_path` 未設定 (`NameDict::empty()`) なら常に空 `Vec` を返す
//! no-op が契約 (§7.10.4.1)。曖昧姓 (`ambiguous=1`) は敬称またはフルネーム
//! 昇格ガード通過が無ければ Detection 自体を生成しない (単独では発火しない)。

use crate::category;
use crate::detection::Detection;
use crate::ja_dict::dict::NameDict;
use crate::ja_dict::{
    CONF_NAME_FULL, CONF_NAME_FULL_HONORIFIC, CONF_NAME_HONORIFIC, CONF_NAME_SURNAME,
};

/// 既定の敬称リスト (設計 §7.10.13)。
const DEFAULT_HONORIFICS: &[&str] = &["様", "さん", "氏", "殿", "君", "先生"];

/// 助詞/機能語ストップリスト (設計 §7.10.6)。姓の直後がこれらで始まる場合、
/// 姓は普通名詞として使われている (例: 「森は」) とみなし昇格しない。
const PARTICLE_STOPWORDS: &[&str] = &[
    "です",
    "でした",
    "という",
    "ばかり",
    "から",
    "まで",
    "は",
    "が",
    "の",
    "を",
    "に",
    "で",
    "と",
];

/// 拡張ネガティブリスト全量 (`docs/resource/ja-name-negative-list.md` を正とする)。
const DEFAULT_NEGATIVE_LIST: &[&str] = &[
    // 会社名接尾辞
    "電機",
    "商事",
    "製作所",
    "工業",
    "興業",
    "商店",
    "製菓",
    "自動車",
    "銀行",
    "信用金庫",
    "証券",
    "不動産",
    // 学術・科学技術
    "理論",
    "学説",
    "モデル",
    "仮説",
    "予想",
    "パラドックス",
    "法則",
    "定理",
    "公式",
    "方程式",
    "関数",
    "曲線",
    "分布",
    "幾何学",
    "力学",
    "効果",
    "現象",
    "プロセス",
    "シミュレーション",
    "チャート",
    "インデックス",
    "スケール",
    "テスト",
    "ライン",
    "バリア",
    // 健康・スポーツ・メソッド・技術
    "体操",
    "運動",
    "メソッド",
    "トレーニング",
    "ストレッチ",
    "ヨガ",
    "ダンス",
    "ステップ",
    "ワーク",
    "式",
    "流",
    "術",
    "法",
    "方式",
    "技法",
    "技術",
    "システム",
    // 社会・経済・思想・トレンド
    "主義",
    "イズム",
    "制度",
    "世代",
    "ブーム",
    "ルート",
    "ビジョン",
    "政策",
    "政権",
    "内閣",
    "改革",
    "対策",
    // 非法人組織・施設・医療
    "クリニック",
    "医院",
    "歯科",
    "整形",
    "眼科",
    "薬局",
    "サロン",
    "スタジオ",
    "スペース",
    "ラボ",
    "研究所",
    "研究会",
    "学会",
    "センター",
    "オフィス",
    "プロジェクト",
    "ゼミ",
    "財団",
    "社団",
    "連盟",
    "協会",
    "連合",
    "機構",
    "ファンド",
    "ギルド",
    // 構造的ストップワード補完
    "自身",
    "本人",
    "周辺",
    "以外",
    "含む",
    "対象",
    "担当",
    "関係",
    "陣営",
    "一派",
];

/// 「名」候補として拾う文字種 (漢字/ひらがな/カタカナ)。
fn is_name_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'
        | '\u{3040}'..='\u{309F}'
        | '\u{30A0}'..='\u{30FF}'
    )
}

/// `pos` の直前に 0〜1 文字の空白/読点があれば読み飛ばした位置を返す。
fn skip_optional_gap(text: &str, pos: usize) -> usize {
    match text[pos..].chars().next() {
        Some(c @ (' ' | '\u{3000}' | '、')) => pos + c.len_utf8(),
        _ => pos,
    }
}

/// `text[pos..]` がリスト中のいずれかの語で始まっていれば、その語のバイト長を返す。
fn match_prefix<'a>(text: &str, words: impl IntoIterator<Item = &'a &'a str>) -> Option<usize> {
    words
        .into_iter()
        .find(|w| text.starts_with(*w))
        .map(|w| w.len())
}

/// `after_gap` から続く「名」候補ラン (敬称の直前で打ち切り) を取り出す。
/// 戻り値はランの終端バイトオフセット (`after_gap` からの絶対値)。空なら `None`。
fn extract_given_name_run(text: &str, after_gap: usize) -> Option<usize> {
    const MAX_RUN_CHARS: usize = 16;
    let mut end = after_gap;
    for (chars, (rel_start, c)) in text[after_gap..].char_indices().enumerate() {
        if chars >= MAX_RUN_CHARS {
            break;
        }
        if match_prefix(&text[after_gap + rel_start..], DEFAULT_HONORIFICS).is_some() {
            break;
        }
        if !is_name_char(c) {
            break;
        }
        end = after_gap + rel_start + c.len_utf8();
    }
    if end == after_gap {
        None
    } else {
        Some(end)
    }
}

/// 敬称/拡張ネガティブリストの前方一致でフルネーム昇格をブロックすべきか。
fn is_guard_blocked(window: &str) -> bool {
    match_prefix(window, PARTICLE_STOPWORDS).is_some()
        || match_prefix(window, DEFAULT_NEGATIVE_LIST).is_some()
}

/// `text` から氏名 (`PII_NAME_JA`) を検出する。`dict` が空 (`NameDict::empty()`)
/// なら常に空 `Vec` を返す (契約、§7.10.4.1)。
pub fn scan(text: &str, dict: &NameDict) -> Vec<Detection> {
    let Some(automaton) = dict.automaton() else {
        return Vec::new();
    };

    let mut detections = Vec::new();
    for m in automaton.find_iter(text) {
        let surname_start = m.start();
        let surname_end = m.end();
        let ambiguous = dict.is_ambiguous(m.pattern().as_usize());

        let after_surname_gap = skip_optional_gap(text, surname_end);
        if let Some(honorific_len) = match_prefix(&text[after_surname_gap..], DEFAULT_HONORIFICS) {
            detections.push(Detection {
                start: surname_start,
                end: after_surname_gap + honorific_len,
                category: category::PII_NAME_JA,
                confidence: CONF_NAME_HONORIFIC,
                rule_id: "ja_name_honorific",
            });
            continue;
        }

        let given_name_end = extract_given_name_run(text, after_surname_gap)
            .filter(|_| !is_guard_blocked(&text[after_surname_gap..]));

        if let Some(run_end) = given_name_end {
            // 検出スパンとしては最大4文字までに切り詰める (設計 §7.10.6)。
            let capped_end = cap_to_chars(text, after_surname_gap, run_end, 4);
            let after_given_gap = skip_optional_gap(text, capped_end);
            if let Some(honorific_len) = match_prefix(&text[after_given_gap..], DEFAULT_HONORIFICS)
            {
                detections.push(Detection {
                    start: surname_start,
                    end: after_given_gap + honorific_len,
                    category: category::PII_NAME_JA,
                    confidence: CONF_NAME_FULL_HONORIFIC,
                    rule_id: "ja_name_full_honorific",
                });
            } else {
                detections.push(Detection {
                    start: surname_start,
                    end: capped_end,
                    category: category::PII_NAME_JA,
                    confidence: CONF_NAME_FULL,
                    rule_id: "ja_name_full",
                });
            }
        } else if !ambiguous {
            detections.push(Detection {
                start: surname_start,
                end: surname_end,
                category: category::PII_NAME_JA,
                confidence: CONF_NAME_SURNAME,
                rule_id: "ja_name_surname",
            });
        }
        // 曖昧姓かつ昇格なしは Detection を生成せず破棄する (§7.10.6)。
    }
    detections
}

/// `text[start..end]` を先頭から最大 `max_chars` 文字に切り詰めた終端オフセットを返す。
fn cap_to_chars(text: &str, start: usize, end: usize, max_chars: usize) -> usize {
    text[start..end]
        .char_indices()
        .nth(max_chars)
        .map(|(rel, _)| start + rel)
        .unwrap_or(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ja_dict::load_surname_dict;
    use std::io::Write;
    use std::path::PathBuf;

    fn write_surname_tsv(name: &str, surname: &str, ambiguous: bool) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "gedoku-detect-ja-name-test-{}-{}-{}",
            std::process::id(),
            name,
            unique
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("surname.tsv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "surname\tambiguous\tfreq_rank").unwrap();
        writeln!(f, "{surname}\t{}\t1", if ambiguous { 1 } else { 0 }).unwrap();
        path
    }

    fn test_dict_with_ambiguous(surname: &str) -> NameDict {
        let path = write_surname_tsv("ambiguous", surname, true);
        load_surname_dict(Some(&path)).expect("test surname tsv must load")
    }

    fn test_dict_non_ambiguous(surname: &str) -> NameDict {
        let path = write_surname_tsv(surname, surname, false);
        load_surname_dict(Some(&path)).expect("test surname tsv must load")
    }

    #[test]
    fn ambiguous_surname_alone_is_not_flagged() {
        let text = "森の中を歩いた";
        let d = scan(text, &test_dict_with_ambiguous("森"));
        assert!(d.is_empty());
    }

    #[test]
    fn ambiguous_surname_with_honorific_is_flagged() {
        let text = "森様にご確認ください";
        let d = scan(text, &test_dict_with_ambiguous("森"));
        let hit = d.iter().find(|x| x.rule_id == "ja_name_honorific").unwrap();
        assert_eq!(hit.text(text), "森様");
        assert!(hit.confidence >= CONF_NAME_HONORIFIC);
    }

    #[test]
    fn negative_list_guards_against_false_full_name() {
        let cases = [
            ("山田電機の株価が上昇した", "山田"),
            ("佐藤理論は物理学の基礎だ", "佐藤"),
            ("佐藤体操を毎朝している", "佐藤"),
            ("田中主義が広まった", "田中"),
            ("山本クリニックを受診した", "山本"),
        ];
        for (text, surname) in cases {
            let d = scan(text, &test_dict_non_ambiguous(surname));
            assert!(
                !d.iter().any(|x| x.rule_id == "ja_name_full"),
                "false positive: {text}"
            );
        }
    }

    #[test]
    fn name_scan_is_noop_when_surname_dict_path_unset() {
        let d = scan("山田太郎様", &NameDict::empty());
        assert!(d.is_empty());
    }

    #[test]
    fn full_name_with_honorific_is_detected() {
        let text = "山田太郎様にご連絡ください";
        let d = scan(text, &test_dict_non_ambiguous("山田"));
        let hit = d
            .iter()
            .find(|x| x.rule_id == "ja_name_full_honorific")
            .unwrap();
        assert_eq!(hit.text(text), "山田太郎様");
        assert!(hit.confidence >= CONF_NAME_FULL_HONORIFIC);
    }
}
