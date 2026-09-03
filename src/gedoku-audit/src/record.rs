//! 監査イベントのスキーマ (設計 §12.2)。
//!
//! P4-T1 の DoD: 「原文/検出値」を格納できるフィールドが型として存在しない。
//! `RedactionRecord`/`RestorationRecord` はカテゴリ別件数・位置情報・非可逆な
//! Realm 参照のみを持ち、`String` 本文を丸ごと保持するフィールドを一切公開
//! しない。呼び出し側はここに定義された構造体のフィールド以外を組み立てて
//! 渡す手段がない。

use std::collections::BTreeMap;

/// Realm ID を直接ログへ出さないための非可逆参照 (§12.2 `realm_ref`)。
/// SHA-256 の先頭 12 桁 (hex) のみを使う。
pub fn realm_ref(realm_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(realm_id.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("r_{}", &hex[..12])
}

/// §12.2 の `redaction` イベント。カテゴリ→件数のみを持つ。
pub struct RedactionRecord<'a> {
    pub realm_id: &'a str,
    pub path: &'a str,
    pub request_bytes_in: usize,
    pub request_bytes_out: usize,
    /// カテゴリ名 (`"SECRET"` 等) → 検出件数。原値は含まない。
    pub counts: BTreeMap<&'static str, u64>,
    /// 一意な原値の個数 (値そのものではなく個数のみ)。
    pub unique_values: usize,
}

pub fn emit_redaction(rec: &RedactionRecord<'_>) {
    let counts_json = serde_json::to_string(&rec.counts).unwrap_or_default();
    tracing::info!(
        target: "gedoku_audit",
        event = "redaction",
        realm_ref = %realm_ref(rec.realm_id),
        path = rec.path,
        request_bytes_in = rec.request_bytes_in,
        request_bytes_out = rec.request_bytes_out,
        counts = %counts_json,
        unique_values = rec.unique_values,
        "gedoku-audit: redaction"
    );
}

/// §12.2 の `restoration` イベント。
pub struct RestorationRecord<'a> {
    pub realm_id: &'a str,
    pub restored: BTreeMap<&'static str, u64>,
    /// 未知/失効プレースホルダー数 (§10.5)。件数のみで ID は含まない。
    pub misses: u64,
}

pub fn emit_restoration(rec: &RestorationRecord<'_>) {
    let restored_json = serde_json::to_string(&rec.restored).unwrap_or_default();
    tracing::info!(
        target: "gedoku_audit",
        event = "restoration",
        realm_ref = %realm_ref(rec.realm_id),
        restored = %restored_json,
        misses = rec.misses,
        "gedoku-audit: restoration"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realm_ref_is_stable_and_non_reversible_looking() {
        let a = realm_ref("cred:abc123");
        let b = realm_ref("cred:abc123");
        let c = realm_ref("cred:different");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(!a.contains("abc123"));
        assert_eq!(a.len(), "r_".len() + 12);
    }
}
