//! AhoCorasick 構築・TSV ローダー (設計 §7.10.3, §7.10.4.2)。
//!
//! 起動時ローダーの全バリアントがここで検証を行い、`JaDictError` を返すのみで
//! パニックしない(唯一の例外は「一意なパターン集合から `AhoCorasick` を構築する」
//! という、このモジュール自身が保証している内部不変条件が破れた場合のみで、
//! これは `rules.rs::re()` が組み込み正規表現のコンパイル失敗で `panic!` するのと
//! 同じ「実装バグでしか起こり得ない」扱い)。

use crate::ja_dict::error::JaDictError;
use aho_corasick::{AhoCorasick, MatchKind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const BUILTIN_PREF_CITY_TSV: &str = include_str!("../../assets/ja/pref_city.tsv");
const BUILTIN_SOURCE_LABEL: &str = "<builtin:assets/ja/pref_city.tsv>";

/// 住所辞書の1エントリの区分 (TSV の `kind` 列)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressKind {
    Pref,
    City,
    Ward,
}

/// 起動時に構築済みの住所オートマトン (設計 §7.10.4.2)。`Arc` 経由で共有借用する想定。
#[derive(Debug)]
pub struct AddressAutomaton {
    automaton: AhoCorasick,
    kinds: Vec<AddressKind>,
    entry_count: usize,
}

impl AddressAutomaton {
    pub(crate) fn automaton(&self) -> &AhoCorasick {
        &self.automaton
    }

    pub(crate) fn kind_of(&self, pattern_id: usize) -> AddressKind {
        self.kinds[pattern_id]
    }

    /// 監査ログ (`ja_pii_dict_loaded`) の `address_entries` フィールド用。
    pub fn pattern_count(&self) -> usize {
        self.entry_count
    }
}

/// 起動時に構築済みの姓辞書 (設計 §7.10.4.2)。`surname_dict_path` 未設定なら
/// [`NameDict::empty`] が渡り、`ja_dict::name::scan` は常に空 `Vec` を返す (no-op 契約)。
#[derive(Debug, Clone)]
pub struct NameDict {
    automaton: Option<AhoCorasick>,
    ambiguous: Vec<bool>,
    entry_count: usize,
}

impl NameDict {
    pub fn empty() -> Self {
        NameDict {
            automaton: None,
            ambiguous: Vec::new(),
            entry_count: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.automaton.is_none()
    }

    /// 監査ログ (`ja_pii_dict_loaded`) の `surname_entries` フィールド用。
    pub fn len(&self) -> usize {
        self.entry_count
    }

    pub(crate) fn automaton(&self) -> Option<&AhoCorasick> {
        self.automaton.as_ref()
    }

    pub(crate) fn is_ambiguous(&self, pattern_id: usize) -> bool {
        self.ambiguous[pattern_id]
    }
}

/// 内蔵 `pref_city.tsv` (+ `extra_path` があれば追加) から `AddressAutomaton` を構築する。
pub fn load_address_dict(extra_path: Option<&Path>) -> Result<AddressAutomaton, JaDictError> {
    let result = load_address_dict_inner(extra_path);
    match &result {
        Ok(automaton) => tracing::info!(
            target: "gedoku_audit",
            event = "ja_pii_dict_loaded",
            dict = "address",
            address_entries = automaton.pattern_count(),
            "ja_pii address dictionary loaded"
        ),
        Err(e) => tracing::error!(
            target: "gedoku_audit",
            event = "ja_pii_dict_load_rejected",
            dict = "address",
            variant = e.variant_name(),
            reason = %e,
            "ja_pii address dictionary load rejected"
        ),
    }
    result
}

fn load_address_dict_inner(extra_path: Option<&Path>) -> Result<AddressAutomaton, JaDictError> {
    let mut names: Vec<String> = Vec::new();
    let mut kinds: Vec<AddressKind> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    parse_address_rows(
        BUILTIN_SOURCE_LABEL,
        BUILTIN_PREF_CITY_TSV,
        &mut names,
        &mut kinds,
        &mut seen,
    )?;

    if let Some(path) = extra_path {
        let raw = read_utf8_file(path)?;
        parse_address_rows(
            &path.display().to_string(),
            &raw,
            &mut names,
            &mut kinds,
            &mut seen,
        )?;
    }

    if names.is_empty() {
        return Err(JaDictError::EmptyDictionary {
            path: extra_path
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(BUILTIN_SOURCE_LABEL)),
        });
    }

    let entry_count = names.len();
    let automaton = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(&names)
        .unwrap_or_else(|e| panic!("gedoku-detect: failed to build address automaton: {e}"));

    Ok(AddressAutomaton {
        automaton,
        kinds,
        entry_count,
    })
}

/// `path` から姓辞書をロードする。`None` なら [`NameDict::empty`] を返す
/// (これがエラーではなく「無効」を表す唯一の経路、設計 §7.10.4.2)。
pub fn load_surname_dict(path: Option<&Path>) -> Result<NameDict, JaDictError> {
    let result = load_surname_dict_inner(path);
    match &result {
        Ok(dict) => tracing::info!(
            target: "gedoku_audit",
            event = "ja_pii_dict_loaded",
            dict = "surname",
            surname_entries = dict.len(),
            "ja_pii surname dictionary loaded"
        ),
        Err(e) => tracing::error!(
            target: "gedoku_audit",
            event = "ja_pii_dict_load_rejected",
            dict = "surname",
            variant = e.variant_name(),
            reason = %e,
            "ja_pii surname dictionary load rejected"
        ),
    }
    result
}

fn load_surname_dict_inner(path: Option<&Path>) -> Result<NameDict, JaDictError> {
    let Some(path) = path else {
        return Ok(NameDict::empty());
    };

    let raw = read_utf8_file(path)?;
    let path_buf = path.to_path_buf();
    let mut names: Vec<String> = Vec::new();
    let mut ambiguous: Vec<bool> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (idx, raw_line) in raw.lines().enumerate() {
        let line_no = idx + 1;
        if line_no == 1 {
            continue; // header: surname\tambiguous\tfreq_rank
        }
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 3 {
            return Err(JaDictError::MalformedRow {
                path: path_buf,
                line: line_no,
                reason: format!("expected 3 tab-separated columns, got {}", cols.len()),
            });
        }
        let surname = cols[0].trim();
        if surname.is_empty() {
            return Err(JaDictError::MalformedRow {
                path: path_buf,
                line: line_no,
                reason: "empty surname".to_string(),
            });
        }
        let is_ambiguous = match cols[1].trim() {
            "0" => false,
            "1" => true,
            other => {
                return Err(JaDictError::MalformedRow {
                    path: path_buf,
                    line: line_no,
                    reason: format!("ambiguous must be 0|1, got {other:?}"),
                })
            }
        };
        if cols[2].trim().parse::<u64>().is_err() {
            return Err(JaDictError::MalformedRow {
                path: path_buf,
                line: line_no,
                reason: format!(
                    "freq_rank must be a non-negative integer, got {:?}",
                    cols[2].trim()
                ),
            });
        }
        if !seen.insert(surname.to_string()) {
            return Err(JaDictError::DuplicateKey {
                path: path_buf,
                line: line_no,
                key: surname.to_string(),
            });
        }
        names.push(surname.to_string());
        ambiguous.push(is_ambiguous);
    }

    if names.is_empty() {
        return Err(JaDictError::EmptyDictionary { path: path_buf });
    }

    let entry_count = names.len();
    let automaton = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(&names)
        .unwrap_or_else(|e| panic!("gedoku-detect: failed to build surname automaton: {e}"));

    Ok(NameDict {
        automaton: Some(automaton),
        ambiguous,
        entry_count,
    })
}

fn parse_address_rows(
    source_label: &str,
    raw: &str,
    names: &mut Vec<String>,
    kinds: &mut Vec<AddressKind>,
    seen: &mut HashSet<String>,
) -> Result<(), JaDictError> {
    let path = PathBuf::from(source_label);
    for (idx, raw_line) in raw.lines().enumerate() {
        let line_no = idx + 1;
        if line_no == 1 {
            continue; // header: name\tkind
        }
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 2 {
            return Err(JaDictError::MalformedRow {
                path,
                line: line_no,
                reason: format!("expected 2 tab-separated columns, got {}", cols.len()),
            });
        }
        let name = cols[0].trim();
        if name.is_empty() {
            return Err(JaDictError::MalformedRow {
                path,
                line: line_no,
                reason: "empty name".to_string(),
            });
        }
        let kind = match cols[1].trim() {
            "pref" => AddressKind::Pref,
            "city" => AddressKind::City,
            "ward" => AddressKind::Ward,
            other => {
                return Err(JaDictError::MalformedRow {
                    path,
                    line: line_no,
                    reason: format!("kind must be pref|city|ward, got {other:?}"),
                })
            }
        };
        if !seen.insert(name.to_string()) {
            return Err(JaDictError::DuplicateKey {
                path,
                line: line_no,
                key: name.to_string(),
            });
        }
        names.push(name.to_string());
        kinds.push(kind);
    }
    Ok(())
}

/// UTF-8 (BOM可) のみ許容する (設計付録 A.8)。それ以外は `InvalidUtf8` で fail_closed。
fn read_utf8_file(path: &Path) -> Result<String, JaDictError> {
    let bytes = std::fs::read(path).map_err(|source| JaDictError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let bytes = strip_bom(&bytes);
    String::from_utf8(bytes.to_vec()).map_err(|_| JaDictError::InvalidUtf8 {
        path: path.to_path_buf(),
    })
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gedoku-detect-ja-dict-test-{}-{}",
            std::process::id(),
            name.replace(['/', '\\'], "_")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn load_address_dict_builds_from_builtin_tsv() {
        let automaton = load_address_dict(None).expect("builtin dict must load");
        assert!(automaton.pattern_count() >= 150);
    }

    #[test]
    fn load_surname_dict_none_is_empty_noop() {
        let dict = load_surname_dict(None).expect("None must be Ok(empty)");
        assert!(dict.is_empty());
        assert_eq!(dict.len(), 0);
    }

    #[test]
    fn load_surname_dict_parses_valid_tsv() {
        let path = write_temp(
            "surname_ok.tsv",
            "surname\tambiguous\tfreq_rank\n山田\t0\t1\n森\t1\t50\n",
        );
        let dict = load_surname_dict(Some(&path)).expect("valid tsv must load");
        assert!(!dict.is_empty());
        assert_eq!(dict.len(), 2);
    }

    #[test]
    fn invalid_surname_tsv_fails_closed_at_startup() {
        let path = write_temp(
            "surname_broken.tsv",
            "surname\tambiguous\tfreq_rank\n山田\t0\n",
        );
        let err = load_surname_dict(Some(&path)).unwrap_err();
        assert!(matches!(err, JaDictError::MalformedRow { .. }));
    }

    #[test]
    fn surname_tsv_with_duplicate_key_is_rejected() {
        let path = write_temp(
            "surname_dup.tsv",
            "surname\tambiguous\tfreq_rank\n山田\t0\t1\n山田\t0\t2\n",
        );
        let err = load_surname_dict(Some(&path)).unwrap_err();
        assert!(matches!(err, JaDictError::DuplicateKey { .. }));
    }

    #[test]
    fn surname_tsv_with_bad_kind_column_count_is_malformed_row() {
        let path = write_temp(
            "surname_cols.tsv",
            "surname\tambiguous\tfreq_rank\n山田\t0\t1\textra\n",
        );
        let err = load_surname_dict(Some(&path)).unwrap_err();
        assert!(matches!(err, JaDictError::MalformedRow { .. }));
    }

    #[test]
    fn empty_surname_tsv_is_rejected_as_empty_dictionary() {
        let path = write_temp("surname_empty.tsv", "surname\tambiguous\tfreq_rank\n");
        let err = load_surname_dict(Some(&path)).unwrap_err();
        assert!(matches!(err, JaDictError::EmptyDictionary { .. }));
    }

    #[test]
    fn missing_surname_file_is_io_error() {
        let err = load_surname_dict(Some(Path::new("does/not/exist-surname.tsv"))).unwrap_err();
        assert!(matches!(err, JaDictError::Io { .. }));
    }

    #[test]
    fn address_dict_rejects_duplicate_key_between_builtin_and_extra() {
        // 内蔵辞書に既にある「東京都」を extra にも入れると重複キーで拒否される。
        let path = write_temp("addr_dup.tsv", "name\tkind\n東京都\tpref\n");
        let err = load_address_dict(Some(&path)).unwrap_err();
        assert!(matches!(err, JaDictError::DuplicateKey { .. }));
    }

    #[test]
    fn address_dict_rejects_unknown_kind() {
        let path = write_temp("addr_bad_kind.tsv", "name\tkind\n架空市\ttown\n");
        let err = load_address_dict(Some(&path)).unwrap_err();
        assert!(matches!(err, JaDictError::MalformedRow { .. }));
    }

    #[test]
    fn address_dict_extra_path_can_add_new_entries() {
        let path = write_temp("addr_extra.tsv", "name\tkind\n架空市\tcity\n");
        let automaton = load_address_dict(Some(&path)).expect("extra dict must merge");
        assert!(automaton.pattern_count() > 150);
    }

    // --- T011: 監査ログ (ja_pii_dict_loaded / ja_pii_dict_load_rejected) ---

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
            .with_max_level(tracing::Level::INFO)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        let captured = buf.lock().unwrap().clone();
        String::from_utf8(captured).unwrap()
    }

    #[test]
    fn ja_pii_dict_loaded_emits_audit_event_with_entry_counts() {
        let log = capture_tracing(|| {
            let _ = load_address_dict(None).expect("builtin dict must load");
        });
        assert!(log.contains("\"event\":\"ja_pii_dict_loaded\""));
        assert!(log.contains("\"target\":\"gedoku_audit\""));
        assert!(log.contains("\"address_entries\""));
    }

    #[test]
    fn ja_pii_dict_load_rejected_covers_all_error_variants() {
        // Io
        let log = capture_tracing(|| {
            let _ = load_surname_dict(Some(Path::new("does/not/exist-surname.tsv")));
        });
        assert!(log.contains("\"event\":\"ja_pii_dict_load_rejected\""));
        assert!(log.contains("\"variant\":\"Io\""));

        // MalformedRow
        let path = write_temp(
            "rejected_malformed.tsv",
            "surname\tambiguous\tfreq_rank\n山田\t0\n",
        );
        let log = capture_tracing(|| {
            let _ = load_surname_dict(Some(&path));
        });
        assert!(log.contains("\"variant\":\"MalformedRow\""));

        // DuplicateKey
        let path = write_temp(
            "rejected_dup.tsv",
            "surname\tambiguous\tfreq_rank\n山田\t0\t1\n山田\t0\t2\n",
        );
        let log = capture_tracing(|| {
            let _ = load_surname_dict(Some(&path));
        });
        assert!(log.contains("\"variant\":\"DuplicateKey\""));

        // EmptyDictionary
        let path = write_temp("rejected_empty.tsv", "surname\tambiguous\tfreq_rank\n");
        let log = capture_tracing(|| {
            let _ = load_surname_dict(Some(&path));
        });
        assert!(log.contains("\"variant\":\"EmptyDictionary\""));

        // InvalidUtf8
        let dir = std::env::temp_dir().join(format!(
            "gedoku-detect-ja-dict-test-{}-rejected_invalid_utf8",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rejected_invalid_utf8.tsv");
        std::fs::write(&path, [0xFFu8, 0xFE, 0xFD]).unwrap();
        let log = capture_tracing(|| {
            let _ = load_surname_dict(Some(&path));
        });
        assert!(log.contains("\"variant\":\"InvalidUtf8\""));
    }
}
