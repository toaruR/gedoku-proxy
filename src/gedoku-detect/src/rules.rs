//! L1: パターン + 検証関数 (設計 §7.4 既定ルールセット)。

use crate::category;
use crate::detection::Detection;
use crate::validators;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

pub struct Rule {
    pub id: &'static str,
    pub category: &'static str,
    pub confidence: f64,
    /// マッチ全体ではなく、この capture group だけを置換対象にする (§7.4)。
    /// `0` はマッチ全体を意味する。
    pub capture_group: usize,
    /// 既定で有効か。false のもの (ipv4_private, abs_path_home) は
    /// `DetectionEngine::extra_enabled_rule_ids` に id を加えない限り発火しない。
    pub enabled_by_default: bool,
    pub pattern: Regex,
    pub validator: Option<fn(&str) -> bool>,
}

fn re(src: &str) -> Regex {
    Regex::new(src).unwrap_or_else(|e| panic!("invalid built-in rule regex {src:?}: {e}"))
}

pub static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        Rule {
            id: "anthropic_key",
            category: category::SECRET,
            confidence: 0.99,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"sk-ant-[A-Za-z0-9_-]{20,}"),
            validator: None,
        },
        Rule {
            id: "openai_key",
            category: category::SECRET,
            confidence: 0.95,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"sk-(?:proj-)?[A-Za-z0-9]{20,}"),
            validator: None,
        },
        Rule {
            id: "aws_akid",
            category: category::SECRET,
            confidence: 0.99,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"(?:AKIA|ASIA)[0-9A-Z]{16}"),
            validator: None,
        },
        Rule {
            id: "aws_secret",
            category: category::SECRET,
            confidence: 0.95,
            capture_group: 1,
            enabled_by_default: true,
            pattern: re(r#"(?i)aws.{0,20}secret.{0,20}['"]([A-Za-z0-9/+=]{40})['"]"#),
            validator: Some(validators::aws_secret_entropy),
        },
        Rule {
            id: "github_pat",
            category: category::SECRET,
            confidence: 0.99,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"gh[pousr]_[A-Za-z0-9]{36,}"),
            validator: Some(validators::github_pat_checksum_valid),
        },
        Rule {
            id: "slack_token",
            category: category::SECRET,
            confidence: 0.97,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"xox[baprs]-[A-Za-z0-9-]{10,}"),
            validator: None,
        },
        Rule {
            id: "gcp_sa_key",
            category: category::SECRET,
            confidence: 0.99,
            capture_group: 1,
            enabled_by_default: true,
            pattern: re(r#""private_key"\s*:\s*"(-----BEGIN[^"]*?-----END[^"]*?-----)""#),
            validator: None,
        },
        Rule {
            id: "pem_private",
            category: category::SECRET,
            confidence: 0.99,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            ),
            validator: None,
        },
        Rule {
            id: "jwt",
            category: category::SECRET,
            confidence: 0.90,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*"),
            validator: Some(validators::jwt_header_valid),
        },
        Rule {
            id: "bearer_ctx",
            category: category::SECRET,
            confidence: 0.85,
            capture_group: 1,
            enabled_by_default: true,
            pattern: re(
                r#"(?i)(?:authorization|bearer|api[_-]?key|token|passwd|password)\s*[:=]\s*(\S{8,})"#,
            ),
            validator: Some(validators::high_entropy_value),
        },
        Rule {
            id: "conn_string",
            category: category::SECRET,
            confidence: 0.95,
            capture_group: 1,
            enabled_by_default: true,
            pattern: re(r"\w+://[^:/\s]+:([^@/\s]+)@"),
            validator: None,
        },
        Rule {
            id: "email",
            category: category::PII_EMAIL,
            confidence: 0.90,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
            validator: Some(validators::email_domain_not_example),
        },
        Rule {
            id: "phone_jp",
            category: category::PII_PHONE,
            confidence: 0.80,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"0\d{1,4}-\d{1,4}-\d{4}"),
            validator: Some(validators::phone_jp_digit_count),
        },
        Rule {
            id: "phone_e164",
            category: category::PII_PHONE,
            confidence: 0.80,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"\+[1-9]\d{7,14}"),
            validator: Some(validators::phone_e164_digit_count),
        },
        Rule {
            id: "credit_card",
            category: category::PII_CARD,
            confidence: 0.97,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"(?:\d[ -]?){13,19}"),
            validator: Some(validators::luhn_valid),
        },
        Rule {
            id: "jp_mynumber",
            category: category::PII_ID,
            confidence: 0.90,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"\b\d{12}\b"),
            validator: Some(validators::mynumber_checkdigit_plausible),
        },
        Rule {
            id: "ssn_us",
            category: category::PII_ID,
            confidence: 0.90,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"\b\d{3}-\d{2}-\d{4}\b"),
            validator: Some(validators::ssn_valid_range),
        },
        Rule {
            id: "iban",
            category: category::PII_ACCOUNT,
            confidence: 0.97,
            capture_group: 0,
            enabled_by_default: true,
            pattern: re(r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b"),
            validator: Some(validators::iban_mod97_valid),
        },
        Rule {
            id: "ipv4_private",
            category: category::NET,
            confidence: 0.50,
            capture_group: 0,
            enabled_by_default: false,
            pattern: re(
                r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[0-1])\.\d{1,3}\.\d{1,3})\b",
            ),
            validator: None,
        },
        Rule {
            id: "abs_path_home",
            category: category::PII_PATH,
            confidence: 0.70,
            capture_group: 1,
            enabled_by_default: false,
            pattern: re(r"/(?:Users|home)/([^/\s]+)"),
            validator: None,
        },
    ]
});

/// L1 スキャン。`extra_enabled` に id を含めた既定 OFF ルールのみ追加で有効化する。
pub fn l1_scan(text: &str, extra_enabled: &HashSet<&'static str>) -> Vec<Detection> {
    let mut out = Vec::new();
    for rule in RULES.iter() {
        if !rule.enabled_by_default && !extra_enabled.contains(rule.id) {
            continue;
        }
        for caps in rule.pattern.captures_iter(text) {
            let Some(m) = caps.get(rule.capture_group) else {
                continue;
            };
            if let Some(validator) = rule.validator {
                if !validator(m.as_str()) {
                    continue;
                }
            }
            out.push(Detection {
                start: m.start(),
                end: m.end(),
                category: rule.category,
                confidence: rule.confidence,
                rule_id: rule.id,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(text: &str) -> Vec<Detection> {
        l1_scan(text, &HashSet::new())
    }

    #[test]
    fn detects_anthropic_key() {
        let text = "here is my key sk-ant-abcdefghijklmnopqrstuvwxyz0123456789 ok";
        let d = scan(text);
        assert!(d.iter().any(|x| x.rule_id == "anthropic_key"));
    }

    #[test]
    fn detects_credit_card_only_when_luhn_valid() {
        let valid = "card 4111 1111 1111 1111 please";
        assert!(scan(valid).iter().any(|x| x.rule_id == "credit_card"));
        let invalid = "card 4111 1111 1111 1112 please";
        assert!(!scan(invalid).iter().any(|x| x.rule_id == "credit_card"));
    }

    #[test]
    fn conn_string_redacts_password_capture_group_only() {
        let text = "postgres://admin:sup3rSecr3t@db.internal:5432/app";
        let d = scan(text);
        let hit = d.iter().find(|x| x.rule_id == "conn_string").unwrap();
        assert_eq!(hit.text(text), "sup3rSecr3t");
    }

    #[test]
    fn email_example_domain_is_not_flagged_by_validator() {
        assert!(!scan("contact noreply@example.com now")
            .iter()
            .any(|x| x.rule_id == "email"));
        assert!(scan("contact real.person@acme-corp.io now")
            .iter()
            .any(|x| x.rule_id == "email"));
    }

    #[test]
    fn optional_rules_require_explicit_opt_in() {
        let text = "internal ip 10.0.0.5 seen in logs";
        assert!(l1_scan(text, &HashSet::new()).is_empty());
        let mut enabled = HashSet::new();
        enabled.insert("ipv4_private");
        assert!(l1_scan(text, &enabled)
            .iter()
            .any(|x| x.rule_id == "ipv4_private"));
    }
}
