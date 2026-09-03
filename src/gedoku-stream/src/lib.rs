//! gedoku-stream: SSE チャンク境界をまたぐプレースホルダー復元 (設計 §12 / 計画 P3)。
//!
//! 他クレートに非依存 (計画 §3 の方針に倣う)。`gedoku-mapstore` のプレースホルダー
//! 文法を byte レベルで再実装し、原文解決は呼び出し側からコールバックで注入する。

pub mod sentinel;
pub mod tailhold;

pub use sentinel::MAX_HOLD_BYTES;
pub use tailhold::TailHoldScanner;
