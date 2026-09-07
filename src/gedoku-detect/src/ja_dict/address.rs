//! 住所検出 (L3' `ja_addr_*`, 設計 §7.10.5)。
//!
//! AhoCorasick で都道府県・市区町村名を検出し (`AddressAutomaton::automaton()`
//! は `MatchKind::LeftmostLongest` で構築済みのため `find_iter` の結果は非重複・
//! 開始位置昇順)、隣接する (間が空文字または半角/全角スペース1文字のみの)
//! アンカーを1つの住所グループへ併合する。グループ直後に番地/丁目/号の
//! 延長パターンがあれば末尾を伸ばし、確信度較正表 (§7.10.7) に従って
//! `rule_id`/`confidence` を決定する。

use crate::category;
use crate::detection::Detection;
use crate::ja_dict::dict::{AddressAutomaton, AddressKind};
use crate::ja_dict::{CONF_ADDR_CITY, CONF_ADDR_FULL, CONF_ADDR_FULL_PREF_ONLY, CONF_ADDR_PREF};
use regex::Regex;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

/// 番地/丁目/号の延長パターン (設計 §7.10.5)。半角数字化 (NFKC) 済みの
/// ウィンドウ文字列に対して適用する。
static BANCHI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:[0-9〇一二三四五六七八九十百千]{1,4}(?:丁目|番地|番|号|-|ー)){1,4}")
        .unwrap_or_else(|e| panic!("invalid built-in ja_addr banchi regex: {e}"))
});

/// 延長探索ウィンドウの最大文字数 (町名等の介在テキストを許容するための余裕)。
const EXTENSION_WINDOW_CHARS: usize = 60;

struct RawAnchor {
    start: usize,
    end: usize,
    kind: AddressKind,
}

/// `text` から住所 (`PII_ADDRESS_JA`) を検出する。
pub fn scan(text: &str, dict: &AddressAutomaton) -> Vec<Detection> {
    let anchors: Vec<RawAnchor> = dict
        .automaton()
        .find_iter(text)
        .map(|m| RawAnchor {
            start: m.start(),
            end: m.end(),
            kind: dict.kind_of(m.pattern().as_usize()),
        })
        .collect();

    let mut detections = Vec::new();
    let mut i = 0;
    while i < anchors.len() {
        let group_start = anchors[i].start;
        let mut group_end = anchors[i].end;
        let mut has_city_or_ward = matches!(anchors[i].kind, AddressKind::City | AddressKind::Ward);

        let mut j = i + 1;
        while j < anchors.len() {
            let gap = &text[group_end..anchors[j].start];
            if !is_mergeable_gap(gap) {
                break;
            }
            group_end = anchors[j].end;
            has_city_or_ward |= matches!(anchors[j].kind, AddressKind::City | AddressKind::Ward);
            j += 1;
        }

        let extension_end = find_banchi_extension(text, group_end);
        let (rule_id, confidence, end) = match (has_city_or_ward, extension_end) {
            (true, Some(ext_end)) => ("ja_addr_full", CONF_ADDR_FULL, ext_end),
            (true, None) => ("ja_addr_city", CONF_ADDR_CITY, group_end),
            (false, Some(ext_end)) => ("ja_addr_full_pref_only", CONF_ADDR_FULL_PREF_ONLY, ext_end),
            (false, None) => ("ja_addr_pref", CONF_ADDR_PREF, group_end),
        };

        detections.push(Detection {
            start: group_start,
            end,
            category: category::PII_ADDRESS_JA,
            confidence,
            rule_id,
        });
        i = j;
    }
    detections
}

/// アンカー間の隙間が併合可能か (空、または半角/全角スペース1文字のみ)。
fn is_mergeable_gap(gap: &str) -> bool {
    gap.is_empty() || gap == " " || gap == "\u{3000}"
}

/// `group_end` 直後の最大 [`EXTENSION_WINDOW_CHARS`] 文字を NFKC 正規化した上で
/// 番地/丁目/号パターンを探し、見つかれば元テキストでの終端バイトオフセットを返す。
fn find_banchi_extension(text: &str, group_end: usize) -> Option<usize> {
    let window = &text[group_end..];

    let mut normalized = String::new();
    // 正規化後の各文字が元テキストのどのバイト範囲 (group_end からの相対オフセット)
    // に対応するかを保持する。1文字が複数文字へ NFKC 分解されるケースも
    // 同じ元範囲を指すエントリを複数積むことで扱う。
    let mut norm_end_to_orig_end: Vec<(usize, usize)> = Vec::new();

    for (count, (start, ch)) in window.char_indices().enumerate() {
        if count >= EXTENSION_WINDOW_CHARS {
            break;
        }
        let orig_end = start + ch.len_utf8();
        for nc in ch.nfkc() {
            normalized.push(nc);
            norm_end_to_orig_end.push((normalized.len(), orig_end));
        }
    }

    let mat = BANCHI_RE.find(&normalized)?;
    let orig_end = norm_end_to_orig_end
        .iter()
        .find(|(norm_end, _)| *norm_end >= mat.end())
        .map(|(_, orig_end)| *orig_end)?;
    Some(group_end + orig_end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ja_dict::load_address_dict;

    fn test_automaton() -> AddressAutomaton {
        load_address_dict(None).expect("builtin dict must load")
    }

    #[test]
    fn ja_addr_full_detected_with_banchi() {
        let text = "東京都港区芝公園4丁目2番8号にお越しください";
        let dict = test_automaton();
        let d = scan(text, &dict);
        let hit = d
            .iter()
            .find(|x| x.rule_id == "ja_addr_full")
            .expect("ja_addr_full must be detected");
        assert_eq!(hit.text(text), "東京都港区芝公園4丁目2番8号");
        assert!(hit.confidence >= 0.90);
    }

    #[test]
    fn bare_city_mention_not_redacted_by_default_engine() {
        let dict = test_automaton();
        let hits = scan("去年は大阪府で花火大会がありました", &dict);
        assert!(hits.iter().all(|d| d.confidence < 0.75));
    }

    #[test]
    fn fullwidth_digits_and_hyphen_are_normalized_via_nfkc() {
        let text = "東京都港区芝公園４丁目２－８にお越しください";
        let dict = test_automaton();
        let d = scan(text, &dict);
        assert!(d
            .iter()
            .any(|x| x.rule_id == "ja_addr_full" && x.confidence >= 0.90));
    }

    #[test]
    fn pref_only_without_banchi_stays_at_pref_confidence() {
        let dict = test_automaton();
        let d = scan("北海道は広い", &dict);
        let hit = d.iter().find(|x| x.rule_id == "ja_addr_pref").unwrap();
        assert!((hit.confidence - CONF_ADDR_PREF).abs() < f64::EPSILON);
    }
}
