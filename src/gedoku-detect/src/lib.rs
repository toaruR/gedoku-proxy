//! gedoku-detect: 検出エンジン (設計 §7, 計画 P2-T1〜T4)。
//!
//! L0 (対象パスの限定) は `gedoku-jsonwalk::collect_scan_targets` の責務。
//! このクレートは「1つのスキャン対象文字列」に対する L1(パターン)/L2(エントロピー)/
//! P1(除外)/P2(重複解決)/P3(閾値) のみを扱い、JSON 構造には非依存。

pub mod allowlist;
pub mod category;
pub mod detection;
pub mod engine;
pub mod entropy;
pub mod ja_dict;
pub mod overlap;
pub mod rules;
pub mod validators;

pub use allowlist::Allowlist;
pub use detection::Detection;
pub use engine::DetectionEngine;
pub use entropy::EntropyConfig;
