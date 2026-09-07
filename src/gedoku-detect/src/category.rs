//! カテゴリ名 (設計 §7.4, §7.7)。
//!
//! 設計上カテゴリは閉じた enum ではなく `[A-Z][A-Z0-9_]{0,23}` の任意文字列
//! (L4 カスタムルールが独自カテゴリを定義できるため)。組み込みルール用に
//! 定数だけ用意し、型としては素の `&'static str` を使う。

pub const SECRET: &str = "SECRET";
pub const PII_EMAIL: &str = "PII_EMAIL";
pub const PII_PHONE: &str = "PII_PHONE";
pub const PII_CARD: &str = "PII_CARD";
pub const PII_ID: &str = "PII_ID";
pub const PII_ACCOUNT: &str = "PII_ACCOUNT";
pub const NET: &str = "NET";
pub const PII_PATH: &str = "PII_PATH";
/// L2 のみが使うヒューリスティックカテゴリ (§7.5)。既定閾値では単独発火しない。
pub const SECRET_HEURISTIC: &str = "SECRET_HEURISTIC";
pub const PII_ADDRESS_JA: &str = "PII_ADDRESS_JA";
pub const PII_NAME_JA: &str = "PII_NAME_JA";
