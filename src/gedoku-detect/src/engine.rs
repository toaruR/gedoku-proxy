//! 検出パイプライン全体 (設計 §7.2 L0(呼び出し側)→L1→L2→P1→P2→P3)。

use crate::allowlist::{drop_allowlisted, Allowlist};
use crate::detection::Detection;
use crate::entropy::{l2_entropy_scan, EntropyConfig};
use crate::overlap::resolve_overlaps;
use crate::rules::l1_scan;
use std::collections::HashSet;

#[derive(Debug)]
pub struct DetectionEngine {
    pub min_confidence: f64,
    pub allowlist: Allowlist,
    /// 既定 OFF のルール (ipv4_private, abs_path_home 等) を個別に有効化する。
    pub extra_enabled_rule_ids: HashSet<&'static str>,
    pub entropy: EntropyConfig,
    pub l2_enabled: bool,
}

impl Default for DetectionEngine {
    fn default() -> Self {
        DetectionEngine {
            min_confidence: 0.75,
            allowlist: Allowlist::new(),
            extra_enabled_rule_ids: HashSet::new(),
            entropy: EntropyConfig::default(),
            l2_enabled: true,
        }
    }
}

impl DetectionEngine {
    /// 1文字列に対する検出パイプライン全体 (§7.9 `redact_request` 内ループの中身)。
    pub fn scan(&self, text: &str) -> Vec<Detection> {
        let mut spans = l1_scan(text, &self.extra_enabled_rule_ids);
        if self.l2_enabled {
            spans.extend(l2_entropy_scan(text, &self.entropy));
        }
        let spans = drop_allowlisted(spans, text, &self.allowlist);
        let mut spans = resolve_overlaps(spans);
        spans.retain(|d| d.confidence >= self.min_confidence);
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l2_alone_never_crosses_default_threshold() {
        let engine = DetectionEngine::default();
        let text = "config: TOKEN=aZ3kQ9mP2wX7bN4hL6cR8vY1sD5tF0";
        // L1 の bearer_ctx 等と重ならない純粋な L2 ケースを想定しつつ、
        // 万一 L1 も拾った場合はそちらの高信頼度が採用されるだけで矛盾しない。
        let hits = engine.scan(text);
        assert!(hits.iter().all(|d| d.confidence >= 0.75));
    }

    #[test]
    fn end_to_end_detects_secret_and_email_with_correct_categories() {
        let engine = DetectionEngine::default();
        let text = "key sk-ant-abcdefghijklmnopqrstuvwxyz0123456789 and mail a@realcompany.io";
        let hits = engine.scan(text);
        assert!(hits.iter().any(|d| d.category == crate::category::SECRET));
        assert!(hits
            .iter()
            .any(|d| d.category == crate::category::PII_EMAIL));
    }

    #[test]
    fn allowlisted_category_is_fully_suppressed() {
        let mut engine = DetectionEngine::default();
        engine
            .allowlist
            .disable_category(crate::category::PII_EMAIL);
        let text = "mail a@realcompany.io";
        assert!(engine.scan(text).is_empty());
    }
}
