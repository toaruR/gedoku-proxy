//! JSON パース/再直列化 (設計 §6.4, 計画 P1-T1/T2)。
//!
//! `serde_json` の `preserve_order` (キー順を `indexmap::IndexMap` で保持) と
//! `arbitrary_precision` (数値トークンを内部的に文字列として保持し `1.50` を
//! `1.5` に丸めない) を有効化することで、パース→無変更再直列化のラウンド
//! トリップが意味的に元バイト列と等価になる (GAP-1 の範囲: バイトレベルの
//! エスケープ様式までは非対応、値の意味的等価性とキー順・数値トークン列・
//! 未知フィールドの保存が対象)。

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JsonWalkError {
    #[error("failed to parse JSON body: {0}")]
    Parse(#[from] serde_json::Error),
}

/// キー順・数値トークンを保持したまま JSON をパースする。
pub fn parse(body: &[u8]) -> Result<Value, JsonWalkError> {
    Ok(serde_json::from_slice(body)?)
}

/// キー順・数値トークンを保持したまま再直列化する (C1-C3, C5 の前提)。
pub fn to_bytes(value: &Value) -> Result<Vec<u8>, JsonWalkError> {
    Ok(serde_json::to_vec(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_key_order() {
        let input = br#"{"z":1,"a":2,"m":3}"#;
        let v = parse(input).unwrap();
        let out = to_bytes(&v).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn preserves_numeric_token_shape() {
        // arbitrary_precision がなければ 1.50 -> 1.5, 1e+10 -> 10000000000.0 等に丸められる。
        // (指数部の符号 `1e10` -> `1e+10` の正規化のみは arbitrary_precision でも起こる。
        // GAP-1: バイト完全一致ではなく数値としての意味的等価性が対象範囲。)
        let input = br#"{"a":1.50,"b":1e+10,"c":9007199254740993}"#;
        let v = parse(input).unwrap();
        let out = to_bytes(&v).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn preserves_unknown_fields() {
        let input = br#"{"known":"x","totally_unknown_future_field":{"nested":true}}"#;
        let v = parse(input).unwrap();
        let out = to_bytes(&v).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn round_trips_unicode_without_reescaping() {
        let input =
            "{\"text\":\"\u{7d75}\u{6587}\u{5b57}\u{7d75}\u{6587}\u{5b57} \u{1f600}\"}".as_bytes();
        let v = parse(input).unwrap();
        let out = to_bytes(&v).unwrap();
        assert_eq!(out, input);
    }
}
