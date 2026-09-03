//! gedoku-audit: 監査ログ + ログ出口ガード (設計 §12 / 計画 P4)。
//!
//! 他クレートに非依存 (計画 §3 の方針に倣う)。値そのものを一切保持しない
//! イベントスキーマ ([`record`]) と、それでも漏れた場合の最後の防壁である
//! Aho-Corasick ベースのログ出口フィルタ ([`guard::LogGuard`]) の2層構成。

pub mod guard;
pub mod record;

pub use guard::LogGuard;
pub use record::{emit_redaction, emit_restoration, realm_ref, RedactionRecord, RestorationRecord};
