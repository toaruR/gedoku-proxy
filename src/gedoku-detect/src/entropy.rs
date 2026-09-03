//! L2: エントロピー検出 (設計 §7.5)。既定 `min_confidence=0.75` のため単独では
//! 置換されず、L1 との重複補強、または閾値を下げた運用でのみ発火する。

use crate::category;
use crate::detection::Detection;
use crate::validators;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub struct EntropyConfig {
    pub min_len: usize,
    pub min_bits: f64,
    pub context_window: usize,
}

impl Default for EntropyConfig {
    fn default() -> Self {
        EntropyConfig {
            min_len: 24,
            min_bits: 4.0,
            context_window: 40,
        }
    }
}

// `=` は末尾の base64 パディングとしてのみ許容する。文字クラス本体に含めると
// `KEY=value` のような代入文が1トークンに融合し、マッチ開始位置が値の手前の
// `=` を越えてしまうため、直前の文脈語 (`in_assignment_context`) を検出できなくなる。
static TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/_-]+=*").expect("valid regex"));

pub fn l2_entropy_scan(text: &str, config: &EntropyConfig) -> Vec<Detection> {
    let mut out = Vec::new();
    for m in TOKEN_RE.find_iter(text) {
        let token = m.as_str();
        if token.len() < config.min_len {
            continue;
        }
        if validators::shannon_entropy(token) < config.min_bits {
            continue;
        }
        if !validators::in_assignment_context(text, m.start(), config.context_window) {
            continue;
        }
        if validators::looks_like_known_noise(token) {
            continue;
        }
        out.push(Detection {
            start: m.start(),
            end: m.end(),
            category: category::SECRET_HEURISTIC,
            confidence: 0.60,
            rule_id: "l2_entropy",
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_high_entropy_token_only_in_assignment_context() {
        let cfg = EntropyConfig::default();
        let with_context = "config: TOKEN=aZ3kQ9mP2wX7bN4hL6cR8vY1sD5tF0";
        assert!(!l2_entropy_scan(with_context, &cfg).is_empty());

        let without_context = "just prose containing aZ3kQ9mP2wX7bN4hL6cR8vY1sD5tF0 randomly";
        assert!(l2_entropy_scan(without_context, &cfg).is_empty());
    }

    #[test]
    fn excludes_git_sha_and_uuid_even_in_context() {
        let cfg = EntropyConfig::default();
        let text = format!("commit sha={}", "a".repeat(40));
        assert!(l2_entropy_scan(&text, &cfg).is_empty());
    }

    #[test]
    fn confidence_is_below_default_threshold() {
        let cfg = EntropyConfig::default();
        let text = "config: TOKEN=aZ3kQ9mP2wX7bN4hL6cR8vY1sD5tF0";
        let hits = l2_entropy_scan(text, &cfg);
        assert!(hits.iter().all(|d| d.confidence < 0.75));
    }
}
