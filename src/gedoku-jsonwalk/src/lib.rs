//! gedoku-jsonwalk: JSON 構造保存 + type-tag ウォーカー (設計 §6, §7, §8 / 計画 P1)。
//!
//! 他クレートに非依存 (計画 §3)。P1 の範囲では検出・置換を一切行わず、
//! 「無検閲のままスキャン対象リーフを漏れなく列挙する」ことのみを担う。

pub mod fieldmap;
pub mod pointer;
pub mod value;

pub use fieldmap::{
    collect_scan_targets, BlockRule, FieldPolicy, FieldRule, FieldShape, ScanTarget,
    StaticBlockRule, BLOCK_RULES,
};
pub use value::{parse, to_bytes, JsonWalkError};
