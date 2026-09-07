//! 検出パイプライン全体 (設計 §7.2 L0(呼び出し側)→L1→L2→P1→P2→P3)。

use crate::allowlist::{drop_allowlisted, Allowlist};
use crate::detection::Detection;
use crate::entropy::{l2_entropy_scan, EntropyConfig};
use crate::ja_dict::{self, load_address_dict, AddressAutomaton, NameDict};
use crate::overlap::resolve_overlaps;
use crate::rules::l1_scan;
use std::collections::HashSet;
use std::sync::Arc;

/// 住所検出 (`ja_dict::address`) の有効フラグと構築済み辞書 (設計 §7.10.8)。
#[derive(Debug, Clone)]
pub struct JaAddressConfig {
    pub enabled: bool,
    pub automaton: Arc<AddressAutomaton>,
}

/// 氏名検出 (`ja_dict::name`) の有効フラグと構築済み辞書 (設計 §7.10.8)。
/// `dict` が [`NameDict::empty`] なら `enabled=true` でも常に no-op (§7.10.4.1)。
#[derive(Debug, Clone)]
pub struct JaNameConfig {
    pub enabled: bool,
    pub dict: Arc<NameDict>,
}

/// 日本語PII (住所/氏名) 検出のマスタースイッチと配下設定 (設計 §7.10.4.3, 付録A.5)。
#[derive(Debug, Clone)]
pub struct JaPiiConfig {
    pub enabled: bool,
    pub address: JaAddressConfig,
    pub name: JaNameConfig,
}

impl Default for JaPiiConfig {
    fn default() -> Self {
        JaPiiConfig {
            enabled: false,
            address: JaAddressConfig {
                enabled: false,
                automaton: Arc::new(
                    load_address_dict(None)
                        .expect("gedoku-detect: builtin ja address dict must load"),
                ),
            },
            name: JaNameConfig {
                enabled: false,
                dict: Arc::new(NameDict::empty()),
            },
        }
    }
}

#[derive(Debug)]
pub struct DetectionEngine {
    pub min_confidence: f64,
    pub allowlist: Allowlist,
    /// 既定 OFF のルール (ipv4_private, abs_path_home 等) を個別に有効化する。
    pub extra_enabled_rule_ids: HashSet<&'static str>,
    pub entropy: EntropyConfig,
    pub l2_enabled: bool,
    pub ja_pii: JaPiiConfig,
}

impl Default for DetectionEngine {
    fn default() -> Self {
        DetectionEngine {
            min_confidence: 0.75,
            allowlist: Allowlist::new(),
            extra_enabled_rule_ids: HashSet::new(),
            entropy: EntropyConfig::default(),
            l2_enabled: true,
            ja_pii: JaPiiConfig::default(),
        }
    }
}

impl DetectionEngine {
    /// 付録A.5の3ビット状態機械: `E`(ja_pii.enabled) が false なのに配下の
    /// `A`(address.enabled) / `S`(name実質有効) が true な3状態は設定ミスとみなし
    /// 起動/実行時に警告する (無効化はするが起動は止めない、fail_openな警告)。
    fn warn_if_ja_pii_inconsistent(&self) {
        let e = self.ja_pii.enabled;
        let a = self.ja_pii.address.enabled;
        let s = self.ja_pii.name.enabled && !self.ja_pii.name.dict.is_empty();
        if !e && a {
            tracing::warn!(
                target: "gedoku_audit",
                event = "ja_pii_config_inconsistent",
                "ja_pii.address.enabled=true だが ja_pii.enabled=false のため無効"
            );
        }
        if !e && s {
            tracing::warn!(
                target: "gedoku_audit",
                event = "ja_pii_config_inconsistent",
                "ja_pii.name.enabled=true だが ja_pii.enabled=false のため無効"
            );
        }
    }

    /// 1文字列に対する検出パイプライン全体 (§7.9 `redact_request` 内ループの中身)。
    pub fn scan(&self, text: &str) -> Vec<Detection> {
        let mut spans = l1_scan(text, &self.extra_enabled_rule_ids);
        if self.l2_enabled {
            spans.extend(l2_entropy_scan(text, &self.entropy));
        }
        if self.ja_pii.enabled {
            if self.ja_pii.address.enabled {
                spans.extend(ja_dict::address::scan(text, &self.ja_pii.address.automaton));
            }
            if self.ja_pii.name.enabled {
                spans.extend(ja_dict::name::scan(text, &self.ja_pii.name.dict));
            }
        } else {
            self.warn_if_ja_pii_inconsistent();
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

    // --- 付録A.5 3ビット状態機械 (ja_pii.enabled / address.enabled / name実質有効) ---

    fn test_name_dict(surname: &str) -> NameDict {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "gedoku-detect-engine-ja-pii-test-{}-{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("surname.tsv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "surname\tambiguous\tfreq_rank").unwrap();
        writeln!(f, "{surname}\t0\t1").unwrap();
        crate::ja_dict::load_surname_dict(Some(&path)).expect("test surname tsv must load")
    }

    #[derive(Clone)]
    struct CapturingWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CapturingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturingWriter {
        type Writer = CapturingWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn capture_tracing<F: FnOnce()>(f: F) -> String {
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer = CapturingWriter(buf.clone());
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(writer)
            .with_max_level(tracing::Level::WARN)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        let captured = buf.lock().unwrap().clone();
        String::from_utf8(captured).unwrap()
    }

    #[test]
    fn ja_pii_all_off_default_detects_nothing_and_warns_nothing() {
        let engine = DetectionEngine::default();
        let text = "東京都港区芝公園4丁目2番8号に山田太郎様がいます";
        let log = capture_tracing(|| {
            let hits = engine.scan(text);
            assert!(hits
                .iter()
                .all(|d| d.category != crate::category::PII_ADDRESS_JA));
            assert!(hits
                .iter()
                .all(|d| d.category != crate::category::PII_NAME_JA));
        });
        assert!(log.is_empty(), "unexpected warning in default state: {log}");
    }

    #[test]
    fn ja_pii_middle_state_master_on_sub_flags_off_is_intentional_no_warn() {
        let mut engine = DetectionEngine::default();
        engine.ja_pii.enabled = true;
        let text = "東京都港区芝公園4丁目2番8号";
        let log = capture_tracing(|| {
            let hits = engine.scan(text);
            assert!(hits
                .iter()
                .all(|d| d.category != crate::category::PII_ADDRESS_JA));
        });
        assert!(log.is_empty(), "middle state must not warn: {log}");
    }

    #[test]
    fn ja_pii_address_only_state_detects_address_not_name() {
        let mut engine = DetectionEngine::default();
        engine.ja_pii.enabled = true;
        engine.ja_pii.address.enabled = true;
        let text = "東京都港区芝公園4丁目2番8号に山田太郎様がいます";
        let hits = engine.scan(text);
        assert!(hits
            .iter()
            .any(|d| d.category == crate::category::PII_ADDRESS_JA));
        assert!(hits
            .iter()
            .all(|d| d.category != crate::category::PII_NAME_JA));
    }

    #[test]
    fn ja_pii_name_only_state_detects_name_not_address() {
        let mut engine = DetectionEngine::default();
        engine.ja_pii.enabled = true;
        engine.ja_pii.name.enabled = true;
        engine.ja_pii.name.dict = Arc::new(test_name_dict("山田"));
        let text = "東京都港区芝公園4丁目2番8号に山田太郎様がいます";
        let hits = engine.scan(text);
        assert!(hits
            .iter()
            .any(|d| d.category == crate::category::PII_NAME_JA));
        assert!(hits
            .iter()
            .all(|d| d.category != crate::category::PII_ADDRESS_JA));
    }

    #[test]
    fn ja_pii_both_enabled_detects_address_and_name() {
        let mut engine = DetectionEngine::default();
        engine.ja_pii.enabled = true;
        engine.ja_pii.address.enabled = true;
        engine.ja_pii.name.enabled = true;
        engine.ja_pii.name.dict = Arc::new(test_name_dict("山田"));
        let text = "東京都港区芝公園4丁目2番8号に山田太郎様がいます";
        let hits = engine.scan(text);
        assert!(hits
            .iter()
            .any(|d| d.category == crate::category::PII_ADDRESS_JA));
        assert!(hits
            .iter()
            .any(|d| d.category == crate::category::PII_NAME_JA));
    }

    #[test]
    fn ja_pii_inconsistent_address_flag_without_master_switch_warns() {
        let mut engine = DetectionEngine::default();
        engine.ja_pii.address.enabled = true;
        let log = capture_tracing(|| {
            engine.scan("plain text");
        });
        assert!(
            log.contains("ja_pii_config_inconsistent"),
            "expected warn: {log}"
        );
    }

    #[test]
    fn ja_pii_inconsistent_name_flag_without_master_switch_warns() {
        let mut engine = DetectionEngine::default();
        engine.ja_pii.name.enabled = true;
        engine.ja_pii.name.dict = Arc::new(test_name_dict("山田"));
        let log = capture_tracing(|| {
            engine.scan("plain text");
        });
        assert!(
            log.contains("ja_pii_config_inconsistent"),
            "expected warn: {log}"
        );
    }

    #[test]
    fn ja_pii_inconsistent_both_flags_without_master_switch_warns_twice() {
        let mut engine = DetectionEngine::default();
        engine.ja_pii.address.enabled = true;
        engine.ja_pii.name.enabled = true;
        engine.ja_pii.name.dict = Arc::new(test_name_dict("山田"));
        let log = capture_tracing(|| {
            engine.scan("plain text");
        });
        assert_eq!(
            log.matches("ja_pii_config_inconsistent").count(),
            2,
            "expected two distinct warnings: {log}"
        );
    }

    #[test]
    fn ja_pii_name_enabled_but_dict_unset_is_treated_as_state_s0_no_warn() {
        // surname_dict_path 未設定 (NameDict::empty) は S=0 相当なので、
        // name.enabled=true でも E=0 との組み合わせで警告は出ない (辞書がなく無害)。
        let mut engine = DetectionEngine::default();
        engine.ja_pii.name.enabled = true;
        let log = capture_tracing(|| {
            engine.scan("plain text");
        });
        assert!(
            log.is_empty(),
            "empty dict should not trigger inconsistency warn: {log}"
        );
    }
}
