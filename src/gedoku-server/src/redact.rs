//! リクエストボディへの検出/置換ワイヤリング (設計 §7.9, 計画 P2-T7)。

use gedoku_core::RealmId;
use gedoku_detect::{Detection, DetectionEngine};
use gedoku_jsonwalk::JsonWalkError;
use gedoku_mapstore::{
    MappingStore, MappingStoreConfig, MemoryMappingStore, Placeholder, PlaceholderId, SecretString,
};
use sha2::{Digest, Sha256};

/// `MappingStore::resolve` は `Placeholder::id` のみをキーに引く (`category` は
/// HMAC 計算にしか使わない、計画 P3)。復元時はカテゴリ情報を持たないため固定値を渡す。
const RESTORE_LOOKUP_CATEGORY: &str = "RESTORE_LOOKUP";

pub struct Redactor {
    engine: DetectionEngine,
    store: MemoryMappingStore,
}

impl Default for Redactor {
    fn default() -> Self {
        Redactor {
            engine: DetectionEngine::default(),
            store: MemoryMappingStore::new(MappingStoreConfig::default()),
        }
    }
}

impl Redactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// §7.9 `redact_request` の実装。L0 (`gedoku-jsonwalk`) が列挙したスキャン対象
    /// リーフのみを検出エンジンにかけ、検出区間をプレースホルダーへ置換する。
    /// 禁止パス (§7.3) はそもそも列挙されないため触れようがない。構造は不変。
    pub fn redact_body(&self, body: &[u8], realm: &RealmId) -> Result<Vec<u8>, JsonWalkError> {
        let mut value = gedoku_jsonwalk::parse(body)?;
        for target in gedoku_jsonwalk::collect_scan_targets(&value) {
            let Some(text) = value.pointer(&target.pointer).and_then(|v| v.as_str()) else {
                continue;
            };
            let detections = self.engine.scan(text);
            if detections.is_empty() {
                continue;
            }
            let redacted = self.substitute(text, &detections, realm);
            if let Some(slot) = value.pointer_mut(&target.pointer) {
                *slot = serde_json::Value::String(redacted);
            }
        }
        gedoku_jsonwalk::to_bytes(&value)
    }

    /// §12: レスポンスストリーム復元 (計画 P3) が使う。プレースホルダー ID から原文を
    /// 引く。未知/失効 ID は `None` (呼び出し側はフェイルオープンでスキャンした生
    /// バイト列をそのまま転送する)。
    pub fn resolve_placeholder(&self, realm: &RealmId, id: &str) -> Option<String> {
        let placeholder = Placeholder {
            category: RESTORE_LOOKUP_CATEGORY,
            id: PlaceholderId::from_scanned(id.to_string()),
        };
        self.store
            .resolve(realm, &placeholder)
            .map(|s| s.expose_secret().to_string())
    }

    fn substitute(&self, text: &str, detections: &[Detection], realm: &RealmId) -> String {
        let mut sorted: Vec<&Detection> = detections.iter().collect();
        sorted.sort_by_key(|d| d.start);
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0usize;
        for d in sorted {
            out.push_str(&text[cursor..d.start]);
            let placeholder = self.store.get_or_create(
                realm,
                d.category,
                SecretString::new(d.text(text).to_string()),
            );
            out.push_str(&placeholder.format(false));
            cursor = d.end;
        }
        out.push_str(&text[cursor..]);
        out
    }
}

/// Realm 導出 (暫定実装)。クライアント資格情報 (`x-api-key` / `Authorization`) を
/// ハッシュ化し Realm 単位に分離することで、「同一クライアントは同一 Realm」という
/// 最小限の分離のみを提供する。session/credential/global/ephemeral の正式な
/// 戦略選択 (設計 §10.4) は P5-T2 で実装する。
pub fn realm_from_headers(headers: &http::HeaderMap) -> RealmId {
    let cred = headers
        .get("x-api-key")
        .or_else(|| headers.get("authorization"))
        .and_then(|v| v.to_str().ok());
    match cred {
        Some(v) => RealmId::new(format!("cred:{}", hex_digest(v.as_bytes()))),
        None => RealmId::new("anonymous"),
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn realm() -> RealmId {
        RealmId::new("test-realm")
    }

    #[test]
    fn redacts_secret_in_scanned_leaf_and_preserves_structure() {
        let redactor = Redactor::new();
        let body = json!({
            "model": "claude-x",
            "messages": [{
                "role": "user",
                "content": "my key is sk-ant-abcdefghijklmnopqrstuvwx, please use it"
            }]
        });
        let out_bytes = redactor
            .redact_body(&serde_json::to_vec(&body).unwrap(), &realm())
            .unwrap();
        let out: serde_json::Value = serde_json::from_slice(&out_bytes).unwrap();
        let text = out["messages"][0]["content"].as_str().unwrap();
        assert!(!text.contains("sk-ant-abcdefghijklmnopqrstuvwx"));
        assert!(text.contains("\u{27E6}GDK:SECRET:"));
        assert_eq!(out["model"], "claude-x");
    }

    #[test]
    fn forbidden_paths_are_never_redacted_even_if_they_look_like_secrets() {
        let redactor = Redactor::new();
        let body = json!({
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_sk-ant-lookalike-id-should-not-be-touched",
                    "name": "run",
                    "input": {"q": "hello"}
                }]
            }]
        });
        let out_bytes = redactor
            .redact_body(&serde_json::to_vec(&body).unwrap(), &realm())
            .unwrap();
        let out: serde_json::Value = serde_json::from_slice(&out_bytes).unwrap();
        assert_eq!(
            out["messages"][0]["content"][0]["id"],
            "toolu_sk-ant-lookalike-id-should-not-be-touched"
        );
    }

    #[test]
    fn same_value_maps_to_same_placeholder_within_realm() {
        let redactor = Redactor::new();
        let body = json!({
            "messages": [
                {"role": "user", "content": "email me at leak@corp-secret.io"},
                {"role": "assistant", "content": "sure, leak@corp-secret.io noted"}
            ]
        });
        let out_bytes = redactor
            .redact_body(&serde_json::to_vec(&body).unwrap(), &realm())
            .unwrap();
        let out: serde_json::Value = serde_json::from_slice(&out_bytes).unwrap();
        let a = out["messages"][0]["content"].as_str().unwrap();
        let b = out["messages"][1]["content"].as_str().unwrap();
        let extract_ph = |s: &str| {
            let after = s.split('\u{27E6}').nth(1).unwrap();
            after.split('\u{27E7}').next().unwrap().to_string()
        };
        assert_eq!(extract_ph(a), extract_ph(b));
    }

    #[test]
    fn no_detections_leaves_body_byte_identical_after_round_trip() {
        let redactor = Redactor::new();
        let input =
            br#"{"model":"claude-x","max_tokens":16,"messages":[{"role":"user","content":"hi"}]}"#;
        let out = redactor.redact_body(input, &realm()).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn realm_from_headers_separates_different_credentials() {
        let mut h1 = http::HeaderMap::new();
        h1.insert("x-api-key", "key-a".parse().unwrap());
        let mut h2 = http::HeaderMap::new();
        h2.insert("x-api-key", "key-b".parse().unwrap());
        assert_ne!(realm_from_headers(&h1), realm_from_headers(&h2));
    }

    #[test]
    fn realm_from_headers_falls_back_to_anonymous_without_credentials() {
        let h = http::HeaderMap::new();
        assert_eq!(realm_from_headers(&h), RealmId::new("anonymous"));
    }
}
