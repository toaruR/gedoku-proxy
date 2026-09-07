//! ja_pii 辞書ローダーの起動時エラー (設計 §7.10.4.2)。

use std::fmt;
use std::path::PathBuf;

/// 辞書ロード失敗の内訳。全バリアントで呼び出し元(`Config::validate` 相当の
/// 起動時検証経路)がプロセス起動を中断する(fail_closed、§18)。
#[derive(Debug)]
pub enum JaDictError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidUtf8 {
        path: PathBuf,
    },
    MalformedRow {
        path: PathBuf,
        line: usize,
        reason: String,
    },
    DuplicateKey {
        path: PathBuf,
        line: usize,
        key: String,
    },
    EmptyDictionary {
        path: PathBuf,
    },
}

impl fmt::Display for JaDictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JaDictError::Io { path, source } => {
                write!(
                    f,
                    "failed to read ja_pii dictionary {}: {source}",
                    path.display()
                )
            }
            JaDictError::InvalidUtf8 { path } => {
                write!(f, "ja_pii dictionary {} is not valid UTF-8", path.display())
            }
            JaDictError::MalformedRow { path, line, reason } => write!(
                f,
                "ja_pii dictionary {} line {line}: malformed row: {reason}",
                path.display()
            ),
            JaDictError::DuplicateKey { path, line, key } => write!(
                f,
                "ja_pii dictionary {} line {line}: duplicate key {key:?}",
                path.display()
            ),
            JaDictError::EmptyDictionary { path } => {
                write!(f, "ja_pii dictionary {} is empty", path.display())
            }
        }
    }
}

impl std::error::Error for JaDictError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            JaDictError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl JaDictError {
    /// `ja_pii_dict_load_rejected` 監査イベントの `variant` フィールド用 (計画T011)。
    pub(crate) fn variant_name(&self) -> &'static str {
        match self {
            JaDictError::Io { .. } => "Io",
            JaDictError::InvalidUtf8 { .. } => "InvalidUtf8",
            JaDictError::MalformedRow { .. } => "MalformedRow",
            JaDictError::DuplicateKey { .. } => "DuplicateKey",
            JaDictError::EmptyDictionary { .. } => "EmptyDictionary",
        }
    }
}
