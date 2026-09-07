//! T012 (設計§7.10.12): §7.10.12記載の8個の単体テストと、P2-T8方式
//! (validator逆算で正例/負例を自動生成しF1をCIゲートにする) を踏襲したja_pii
//! 用コーパステスト。学習側・ホールドアウト側(10%)双方でF1>=0.90をCIゲートとする。
//!
//! ホールドアウトの構成方針 (付録A.4対抗策#1):
//! - 氏名: 学習用姓リストとホールドアウト用姓リストを完全に分離した2つの
//!   `NameDict` を別々に構築し、双方で同じアルゴリズム(name::scan)がF1劣化なく
//!   機能することを確認する(姓辞書は本テスト専用のダミーデータであり、
//!   アルゴリズム側に特定の姓文字列がハードコードされていないことの回帰検証)。
//! - 住所: 内蔵`pref_city.tsv`は全件が常にオートマトンへ含まれる(部分的な
//!   辞書構築APIが無い)ため、コーパス生成時にのみ「学習側で使う市区町村名」
//!   と「ホールドアウト側でのみ使う市区町村名」を分離し、同一の完全な
//!   オートマトンに対して両側のテキストを走査する。これにより「テストコード
//!   中に特定の市区町村名だけがハードコードされている」ことによる見かけ上の
//!   高スコアを防ぐ。

use gedoku_detect::category;
use gedoku_detect::engine::DetectionEngine;
use gedoku_detect::ja_dict::{self, load_address_dict, load_surname_dict, JaDictError, NameDict};
use std::path::Path;

// --- §7.10.12 記載の8個の単体テスト (設計書コードブロックそのまま) ---

fn test_automaton() -> ja_dict::AddressAutomaton {
    load_address_dict(None).expect("builtin dict must load")
}

fn write_surname_tsv(tag: &str, surname: &str, ambiguous: bool) -> std::path::PathBuf {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gedoku-detect-corpus-test-{}-{tag}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("surname.tsv");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "surname\tambiguous\tfreq_rank").unwrap();
    writeln!(f, "{surname}\t{}\t1", if ambiguous { 1 } else { 0 }).unwrap();
    path
}

fn test_dict_with_ambiguous(surname: &str) -> NameDict {
    load_surname_dict(Some(&write_surname_tsv("ambiguous", surname, true)))
        .expect("test surname tsv must load")
}

fn test_dict_non_ambiguous(surname: &str) -> NameDict {
    load_surname_dict(Some(&write_surname_tsv("nonambig", surname, false)))
        .expect("test surname tsv must load")
}

#[test]
fn ja_addr_full_detected_with_banchi() {
    let text = "東京都港区芝公園4丁目2番8号にお越しください";
    let d = ja_dict::address::scan(text, &test_automaton());
    let hit = d.iter().find(|x| x.rule_id == "ja_addr_full").unwrap();
    assert_eq!(hit.text(text), "東京都港区芝公園4丁目2番8号");
    assert!(hit.confidence >= 0.90);
}

#[test]
fn bare_city_mention_not_redacted_by_default_engine() {
    let mut engine = DetectionEngine::default();
    engine.ja_pii.enabled = true;
    engine.ja_pii.address.enabled = true;
    let hits = engine.scan("去年は大阪府で花火大会がありました");
    assert!(!hits.iter().any(|d| d.category == category::PII_ADDRESS_JA));
}

#[test]
fn ambiguous_surname_alone_is_not_flagged() {
    let text = "森の中を歩いた";
    let d = ja_dict::name::scan(text, &test_dict_with_ambiguous("森"));
    assert!(d.is_empty());
}

#[test]
fn ambiguous_surname_with_honorific_is_flagged() {
    let text = "森様にご確認ください";
    let d = ja_dict::name::scan(text, &test_dict_with_ambiguous("森"));
    let hit = d.iter().find(|x| x.rule_id == "ja_name_honorific").unwrap();
    assert_eq!(hit.text(text), "森様");
}

#[test]
fn negative_list_guards_against_false_full_name() {
    let cases = [
        ("山田電機の株価が上昇した", "山田"), // 会社名接尾辞
        ("佐藤理論は物理学の基礎だ", "佐藤"), // 学術・科学技術
        ("佐藤体操を毎朝している", "佐藤"),   // 健康・技法
        ("田中主義が広まった", "田中"),       // 社会・思想
        ("山本クリニックを受診した", "山本"), // 非法人組織・医療
    ];
    for (text, surname) in cases {
        let d = ja_dict::name::scan(text, &test_dict_non_ambiguous(surname));
        assert!(
            !d.iter().any(|x| x.rule_id == "ja_name_full"),
            "false positive: {text}"
        );
    }
}

#[test]
fn name_scan_is_noop_when_surname_dict_path_unset() {
    let d = ja_dict::name::scan("山田太郎様", &NameDict::empty());
    assert!(d.is_empty());
}

#[test]
fn invalid_surname_tsv_fails_closed_at_startup() {
    let err = load_surname_dict(Some(Path::new("does/not/exist-broken.tsv"))).unwrap_err();
    assert!(matches!(err, JaDictError::Io { .. }));
}

#[test]
fn addr_full_does_not_collide_with_jp_mynumber_regex() {
    // §7.10.9: 丁目/番地表記は漢字区切りのため12桁純数字のregexとは字面上一致しない。
    let mut engine = DetectionEngine::default();
    engine.ja_pii.enabled = true;
    engine.ja_pii.address.enabled = true;
    let text = "港区123456789012丁目1番1号"; // 意図的に12桁の羅列を隣接させる回帰テスト
    let hits = engine.scan(text);
    assert!(hits.iter().filter(|x| x.rule_id == "jp_mynumber").count() <= 1);
}

// --- コーパスF1テスト (P2-T8方式踏襲) ---

const TRAIN_CITIES: &[&str] = &[
    "札幌市",
    "仙台市",
    "さいたま市",
    "千葉市",
    "横浜市",
    "川崎市",
    "相模原市",
    "新潟市",
    "静岡市",
    "浜松市",
    "名古屋市",
    "京都市",
    "大阪市",
    "堺市",
    "神戸市",
    "岡山市",
    "広島市",
    "北九州市",
    "福岡市",
    "熊本市",
    "青森市",
    "盛岡市",
    "秋田市",
    "山形市",
    "福島市",
    "水戸市",
    "宇都宮市",
    "富山市",
    "金沢市",
    "福井市",
];

/// 学習コーパス生成では一度も参照しない市区町村名 (10%規模)。
const HOLDOUT_CITIES: &[&str] = &["長崎市", "大分市", "宮崎市", "鹿児島市"];

const TRAIN_PREFS: &[&str] = &["北海道", "大阪府", "京都府", "沖縄県"];

/// 「学習」用姓辞書 (本テスト専用のダミーデータ)。`林` のみ ambiguous=1。
const TRAIN_SURNAMES: &[(&str, bool)] = &[
    ("山田", false),
    ("佐藤", false),
    ("鈴木", false),
    ("田中", false),
    ("伊藤", false),
    ("渡辺", false),
    ("中村", false),
    ("小林", false),
    ("加藤", false),
    ("吉田", false),
    ("山本", false),
    ("松本", false),
    ("井上", false),
    ("木村", false),
    ("林", true),
    ("清水", false),
    ("山口", false),
    ("森田", false),
];

/// 学習コーパス生成では一度も参照しないダミー姓 (10%規模、専用辞書で評価する)。
const HOLDOUT_SURNAMES: &[(&str, bool)] = &[("斎藤", false), ("岡田", false)];

const GIVEN_NAMES: &[&str] = &[
    "太郎", "花子", "次郎", "三郎", "恵子", "一郎", "直子", "健二", "由美", "拓也",
];

/// 拡張ネガティブリスト5カテゴリの代表語 (`ja_dict::name`のDEFAULT_NEGATIVE_LISTと同期)。
const NEGATIVE_WORDS: &[&str] = &["電機", "理論", "体操", "主義", "クリニック"];

fn build_name_dict(entries: &[(&str, bool)], tag: &str) -> NameDict {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!(
        "gedoku-detect-corpus-namedict-{}-{tag}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("surname.tsv");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "surname\tambiguous\tfreq_rank").unwrap();
    for (i, (surname, ambiguous)) in entries.iter().enumerate() {
        writeln!(f, "{surname}\t{}\t{i}", if *ambiguous { 1 } else { 0 }).unwrap();
    }
    load_surname_dict(Some(&path)).expect("corpus surname tsv must load")
}

fn engine_with_name_dict(dict: NameDict) -> DetectionEngine {
    let mut engine = DetectionEngine::default();
    engine.ja_pii.enabled = true;
    engine.ja_pii.address.enabled = true;
    engine.ja_pii.name.enabled = true;
    engine.ja_pii.name.dict = std::sync::Arc::new(dict);
    engine
}

/// 正例(期待カテゴリ, テキスト)と負例(検出されてはならないテキスト)を
/// `cities`/`surnames`(非曖昧のみを氏名フル/敬称の生成対象とする) から合成する。
fn build_corpus(
    cities: &[&str],
    prefs: &[&str],
    surnames: &[(&str, bool)],
) -> (Vec<(&'static str, String)>, Vec<String>) {
    let mut positives: Vec<(&'static str, String)> = Vec::new();
    let mut negatives: Vec<String> = Vec::new();

    for (i, city) in cities.iter().enumerate() {
        let n = 1 + i % 9;
        let m = 1 + i % 5;
        let k = 1 + i % 8;
        positives.push((
            category::PII_ADDRESS_JA,
            format!("{city}{n}丁目{m}番{k}号に行きます"),
        ));
        // 負例: 番地なしの裸の市区町村言及 (閾値0.75未満で検出されないべき)。
        negatives.push(format!("去年は{city}で花火大会がありました"));
    }

    for pref in prefs {
        // 負例: 都道府県名のみ (閾値0.75未満)。
        negatives.push(format!("{pref}は広い"));
    }

    for (i, (surname, ambiguous)) in surnames.iter().enumerate() {
        let given = GIVEN_NAMES[i % GIVEN_NAMES.len()];
        positives.push((
            category::PII_NAME_JA,
            format!("{surname}{given}について話した"),
        ));
        positives.push((
            category::PII_NAME_JA,
            format!("{surname}様にご確認ください"),
        ));
        positives.push((
            category::PII_NAME_JA,
            format!("{surname}{given}様にご連絡ください"),
        ));

        if *ambiguous {
            // 負例: 曖昧姓単独 (昇格なしは破棄されるべき)。
            negatives.push(format!("{surname}の中を歩いた"));
        } else {
            // 負例: 非曖昧姓単独 (ja_name_surname=0.65は閾値未満で検出されないべき)。
            negatives.push(format!("{surname}の近くで見かけた"));
        }

        // 負例: 拡張ネガティブリスト5カテゴリとの組み合わせ。
        let neg_word = NEGATIVE_WORDS[i % NEGATIVE_WORDS.len()];
        negatives.push(format!("{surname}{neg_word}についての記事を読んだ"));

        // 負例: 助詞ストップワード直後 (普通名詞としての用法)。
        negatives.push(format!("{surname}は忙しいらしい"));
    }

    (positives, negatives)
}

struct Report {
    recall: f64,
    precision: f64,
    f1: f64,
    false_positive_rate: f64,
}

fn evaluate(
    engine: &DetectionEngine,
    positives: &[(&'static str, String)],
    negatives: &[String],
) -> Report {
    let mut true_positive = 0usize;
    for (expected_category, text) in positives {
        let hits = engine.scan(text);
        if hits.iter().any(|d| d.category == *expected_category) {
            true_positive += 1;
        } else {
            eprintln!("MISS[{expected_category}]: {text}");
        }
    }

    let mut false_positive_texts = 0usize;
    let mut false_positive_detections = 0usize;
    for text in negatives {
        let hits: Vec<_> = engine
            .scan(text)
            .into_iter()
            .filter(|d| {
                d.category == category::PII_ADDRESS_JA || d.category == category::PII_NAME_JA
            })
            .collect();
        if !hits.is_empty() {
            false_positive_texts += 1;
            false_positive_detections += hits.len();
            eprintln!("FALSE POSITIVE: {text} -> {hits:?}");
        }
    }

    let recall = true_positive as f64 / positives.len() as f64;
    let false_positive_rate = false_positive_texts as f64 / negatives.len() as f64;
    let precision =
        true_positive as f64 / (true_positive + false_positive_detections).max(1) as f64;
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };

    Report {
        recall,
        precision,
        f1,
        false_positive_rate,
    }
}

#[test]
fn ja_pii_corpus_f1_report() {
    let train_engine = engine_with_name_dict(build_name_dict(TRAIN_SURNAMES, "train"));
    let (train_positives, train_negatives) =
        build_corpus(TRAIN_CITIES, TRAIN_PREFS, TRAIN_SURNAMES);
    assert!(train_positives.len() >= 40, "train positives too small");
    assert!(train_negatives.len() >= 40, "train negatives too small");
    let train_report = evaluate(&train_engine, &train_positives, &train_negatives);
    println!(
        "ja_pii train corpus report: N_pos={} N_neg={} recall={:.4} precision={:.4} f1={:.4} fp_rate={:.4}",
        train_positives.len(),
        train_negatives.len(),
        train_report.recall,
        train_report.precision,
        train_report.f1,
        train_report.false_positive_rate
    );
    assert!(
        train_report.f1 >= 0.90,
        "train f1 too low: {:.4}",
        train_report.f1
    );

    let holdout_engine = engine_with_name_dict(build_name_dict(HOLDOUT_SURNAMES, "holdout"));
    let (holdout_positives, holdout_negatives) =
        build_corpus(HOLDOUT_CITIES, &[], HOLDOUT_SURNAMES);
    let holdout_report = evaluate(&holdout_engine, &holdout_positives, &holdout_negatives);
    println!(
        "ja_pii holdout corpus report: N_pos={} N_neg={} recall={:.4} precision={:.4} f1={:.4} fp_rate={:.4}",
        holdout_positives.len(),
        holdout_negatives.len(),
        holdout_report.recall,
        holdout_report.precision,
        holdout_report.f1,
        holdout_report.false_positive_rate
    );
    assert!(
        holdout_report.f1 >= 0.90,
        "holdout f1 too low (memorization suspected): {:.4}",
        holdout_report.f1
    );
    assert!(
        (train_report.f1 - holdout_report.f1).abs() <= 0.10,
        "train/holdout f1 diverge too much: train={:.4} holdout={:.4}",
        train_report.f1,
        holdout_report.f1
    );
}
