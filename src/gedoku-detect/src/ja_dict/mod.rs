//! 日本語PII (住所・氏名) 検出用の辞書ロード・オートマトン (設計 §7.10)。

pub mod address;
mod dict;
pub mod error;
pub mod name;

pub use dict::{load_address_dict, load_surname_dict, AddressAutomaton, AddressKind, NameDict};
pub use error::JaDictError;

/// 確信度較正表 (設計 §7.10.7)。テストが本表に対して一致検証を行う唯一の真実源。
pub(crate) const CONF_ADDR_PREF: f64 = 0.50;
pub(crate) const CONF_ADDR_CITY: f64 = 0.55;
pub(crate) const CONF_ADDR_FULL: f64 = 0.90;
pub(crate) const CONF_ADDR_FULL_PREF_ONLY: f64 = 0.85;
pub(crate) const CONF_NAME_SURNAME: f64 = 0.65;
pub(crate) const CONF_NAME_FULL: f64 = 0.90;
pub(crate) const CONF_NAME_HONORIFIC: f64 = 0.92;
pub(crate) const CONF_NAME_FULL_HONORIFIC: f64 = 0.97;
