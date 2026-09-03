//! P1: 除外リスト (設計 §7.6)。JSON Pointer による除外は L0 (gedoku-jsonwalk) の
//! 責務なのでここでは扱わない (二重防壁の片側のみ)。

use crate::detection::Detection;
use regex::Regex;
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct Allowlist {
    literals: HashSet<String>,
    patterns: Vec<Regex>,
    domains: HashSet<String>,
    disabled_categories: HashSet<&'static str>,
}

impl Allowlist {
    pub fn new() -> Self {
        Allowlist::default()
    }

    pub fn add_literal(&mut self, value: impl Into<String>) -> &mut Self {
        self.literals.insert(value.into());
        self
    }

    pub fn add_pattern(&mut self, pattern: Regex) -> &mut Self {
        self.patterns.push(pattern);
        self
    }

    pub fn add_domain(&mut self, domain: impl Into<String>) -> &mut Self {
        self.domains.insert(domain.into().to_ascii_lowercase());
        self
    }

    pub fn disable_category(&mut self, category: &'static str) -> &mut Self {
        self.disabled_categories.insert(category);
        self
    }

    pub fn is_allowed(&self, category: &str, matched_text: &str) -> bool {
        if self.disabled_categories.contains(category) {
            return true;
        }
        if self.literals.contains(matched_text) {
            return true;
        }
        if self.patterns.iter().any(|p| p.is_match(matched_text)) {
            return true;
        }
        if !self.domains.is_empty() {
            if let Some((_, domain)) = matched_text.rsplit_once('@') {
                if self.domains.contains(&domain.to_ascii_lowercase()) {
                    return true;
                }
            }
        }
        false
    }
}

pub fn drop_allowlisted(
    detections: Vec<Detection>,
    text: &str,
    allow: &Allowlist,
) -> Vec<Detection> {
    detections
        .into_iter()
        .filter(|d| !allow.is_allowed(d.category, d.text(text)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category;

    fn detection(category: &'static str, start: usize, end: usize) -> Detection {
        Detection {
            start,
            end,
            category,
            confidence: 0.9,
            rule_id: "test",
        }
    }

    #[test]
    fn literal_allowlist_drops_exact_match() {
        let text = "noreply@example.com";
        let mut allow = Allowlist::new();
        allow.add_literal("noreply@example.com");
        let dets = vec![detection(category::PII_EMAIL, 0, text.len())];
        assert!(drop_allowlisted(dets, text, &allow).is_empty());
    }

    #[test]
    fn category_disable_drops_all_of_that_category() {
        let text = "10.0.0.5";
        let mut allow = Allowlist::new();
        allow.disable_category(category::NET);
        let dets = vec![detection(category::NET, 0, text.len())];
        assert!(drop_allowlisted(dets, text, &allow).is_empty());
    }

    #[test]
    fn domain_allowlist_drops_matching_email_domain() {
        let text = "a@internal-test.local";
        let mut allow = Allowlist::new();
        allow.add_domain("internal-test.local");
        let dets = vec![detection(category::PII_EMAIL, 0, text.len())];
        assert!(drop_allowlisted(dets, text, &allow).is_empty());
    }

    #[test]
    fn unrelated_detection_is_kept() {
        let text = "a@realcompany.io";
        let allow = Allowlist::new();
        let dets = vec![detection(category::PII_EMAIL, 0, text.len())];
        assert_eq!(drop_allowlisted(dets, text, &allow).len(), 1);
    }
}
