//! マッピングストア: AES-256-GCM 暗号化 + Realm 分離 + TTL/LRU 退避
//! (設計 §10.3〜§10.5, 計画 P2-T5/T6)。

use crate::harden::mlock_best_effort;
use crate::placeholder::{
    derive_enc_key, derive_realm_pepper, normalize, placeholder_id, Placeholder,
};
use crate::secret::SecretString;
use aes_gcm::aead::{Aead, Generate, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use gedoku_core::RealmId;
use lru::LruCache;
use rand::RngCore;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, thiserror::Error)]
pub enum MapStoreError {
    #[error("failed to encrypt mapping entry")]
    EncryptionFailed,
}

/// §10.5 TTL/容量パラメータ。
#[derive(Debug, Clone)]
pub struct MappingStoreConfig {
    pub realm_idle_ttl: Duration,
    pub realm_max_ttl: Duration,
    pub realm_max_entries: usize,
    pub global_max_realms: usize,
}

impl Default for MappingStoreConfig {
    fn default() -> Self {
        MappingStoreConfig {
            realm_idle_ttl: Duration::from_secs(30 * 60),
            realm_max_ttl: Duration::from_secs(12 * 60 * 60),
            realm_max_entries: 10_000,
            global_max_realms: 2_000,
        }
    }
}

/// 計画 §3.1 の境界 trait。バックエンド (memory/redis 等、§10.6) を差し替え可能にする。
pub trait MappingStore: Send + Sync {
    fn put(
        &self,
        realm: &RealmId,
        placeholder: &Placeholder,
        original: SecretString,
    ) -> Result<(), MapStoreError>;
    fn resolve(&self, realm: &RealmId, placeholder: &Placeholder) -> Option<SecretString>;
    fn evict_zeroed(&self, realm: &RealmId);
}

struct EncryptedEntry {
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

impl Drop for EncryptedEntry {
    fn drop(&mut self) {
        self.ciphertext.zeroize();
    }
}

struct RealmState {
    entries: LruCache<crate::placeholder::PlaceholderId, EncryptedEntry>,
    created_at: Instant,
    last_access: Instant,
}

/// 既定バックエンド (§10.3 M1: プロセスメモリにのみ置く)。プロセス終了で全消失する (M7)。
pub struct MemoryMappingStore {
    master_key: Zeroizing<[u8; 32]>,
    realms: Mutex<HashMap<RealmId, RealmState>>,
    config: MappingStoreConfig,
}

impl MemoryMappingStore {
    pub fn new(config: MappingStoreConfig) -> Self {
        let mut key = [0u8; 32];
        rand::rng().fill_bytes(&mut key);
        Self::with_master_key(key, config)
    }

    /// テスト・将来の鍵ローテーション用に、既知の鍵で構築する。
    pub fn with_master_key(master_key: [u8; 32], config: MappingStoreConfig) -> Self {
        let master_key = Zeroizing::new(master_key);
        mlock_best_effort(master_key.as_slice());
        MemoryMappingStore {
            master_key,
            realms: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// §7.8, §7.9 `mapping_store.get_or_create`: 同一 Realm・同一値なら常に同じ
    /// Placeholder を返す (決定論)。未存在なら暗号化して新規登録する。
    pub fn get_or_create(
        &self,
        realm: &RealmId,
        category: &'static str,
        original: SecretString,
    ) -> Placeholder {
        let pepper = derive_realm_pepper(&self.master_key[..], realm);
        let normalized = normalize(category, original.expose_secret());
        let id = placeholder_id(&pepper, category, &normalized);
        let placeholder = Placeholder {
            category,
            id: id.clone(),
        };

        let mut realms = self.realms.lock().expect("mapstore mutex poisoned");
        self.evict_expired_locked(&mut realms);
        let now = Instant::now();
        let entries_cap =
            NonZeroUsize::new(self.config.realm_max_entries).unwrap_or(NonZeroUsize::MIN);
        let state = realms.entry(realm.clone()).or_insert_with(|| RealmState {
            entries: LruCache::new(entries_cap),
            created_at: now,
            last_access: now,
        });
        state.last_access = now;

        if state.entries.get(&id).is_none() {
            let enc_key = derive_enc_key(&self.master_key[..], realm);
            if let Some((nonce, ciphertext)) =
                encrypt(&enc_key, original.expose_secret().as_bytes())
            {
                state.entries.put(id, EncryptedEntry { nonce, ciphertext });
            }
        }
        self.evict_global_overflow_locked(&mut realms);
        placeholder
    }

    fn evict_expired_locked(&self, realms: &mut HashMap<RealmId, RealmState>) {
        let now = Instant::now();
        realms.retain(|_, state| {
            now.duration_since(state.last_access) < self.config.realm_idle_ttl
                && now.duration_since(state.created_at) < self.config.realm_max_ttl
        });
    }

    fn evict_global_overflow_locked(&self, realms: &mut HashMap<RealmId, RealmState>) {
        while realms.len() > self.config.global_max_realms {
            let Some(oldest) = realms
                .iter()
                .min_by_key(|(_, s)| s.last_access)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            realms.remove(&oldest);
        }
    }
}

impl MappingStore for MemoryMappingStore {
    fn put(
        &self,
        realm: &RealmId,
        placeholder: &Placeholder,
        original: SecretString,
    ) -> Result<(), MapStoreError> {
        let mut realms = self.realms.lock().expect("mapstore mutex poisoned");
        self.evict_expired_locked(&mut realms);
        let now = Instant::now();
        let entries_cap =
            NonZeroUsize::new(self.config.realm_max_entries).unwrap_or(NonZeroUsize::MIN);
        let state = realms.entry(realm.clone()).or_insert_with(|| RealmState {
            entries: LruCache::new(entries_cap),
            created_at: now,
            last_access: now,
        });
        state.last_access = now;

        let enc_key = derive_enc_key(&self.master_key[..], realm);
        let (nonce, ciphertext) = encrypt(&enc_key, original.expose_secret().as_bytes())
            .ok_or(MapStoreError::EncryptionFailed)?;
        state
            .entries
            .put(placeholder.id.clone(), EncryptedEntry { nonce, ciphertext });
        drop(realms);
        let mut realms = self.realms.lock().expect("mapstore mutex poisoned");
        self.evict_global_overflow_locked(&mut realms);
        Ok(())
    }

    fn resolve(&self, realm: &RealmId, placeholder: &Placeholder) -> Option<SecretString> {
        let mut realms = self.realms.lock().expect("mapstore mutex poisoned");
        self.evict_expired_locked(&mut realms);
        let state = realms.get_mut(realm)?;
        state.last_access = Instant::now();
        let entry = state.entries.get(&placeholder.id)?;
        let enc_key = derive_enc_key(&self.master_key[..], realm);
        let plaintext = decrypt(&enc_key, &entry.nonce, &entry.ciphertext)?;
        let s = String::from_utf8(plaintext.to_vec()).ok()?;
        Some(SecretString::new(s))
    }

    /// §10.3 M6: 退避時ゼロ化。`EncryptedEntry` の `Drop` が暗号文バッファをゼロ埋めする。
    fn evict_zeroed(&self, realm: &RealmId) {
        let mut realms = self.realms.lock().expect("mapstore mutex poisoned");
        realms.remove(realm);
    }
}

fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Option<([u8; 12], Vec<u8>)> {
    let key = Key::<Aes256Gcm>::try_from(key.as_slice()).ok()?;
    let cipher = Aes256Gcm::new(&key);
    let nonce = Nonce::generate();
    let ciphertext = cipher.encrypt(&nonce, plaintext).ok()?;
    let mut nonce_bytes = [0u8; 12];
    nonce_bytes.copy_from_slice(nonce.as_slice());
    Some((nonce_bytes, ciphertext))
}

fn decrypt(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let key = Key::<Aes256Gcm>::try_from(key.as_slice()).ok()?;
    let cipher = Aes256Gcm::new(&key);
    let nonce = Nonce::try_from(nonce.as_slice()).ok()?;
    cipher.decrypt(&nonce, ciphertext).ok().map(Zeroizing::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryMappingStore {
        MemoryMappingStore::with_master_key([9u8; 32], MappingStoreConfig::default())
    }

    #[test]
    fn get_or_create_is_deterministic_within_same_realm() {
        let store = store();
        let realm = RealmId::new("s1");
        let p1 = store.get_or_create(&realm, "PII_EMAIL", SecretString::new("a@x.io".into()));
        let p2 = store.get_or_create(&realm, "PII_EMAIL", SecretString::new("a@x.io".into()));
        assert_eq!(p1, p2);
    }

    #[test]
    fn resolve_returns_original_value_via_get_or_create_placeholder() {
        let store = store();
        let realm = RealmId::new("s1");
        let placeholder = store.get_or_create(
            &realm,
            "SECRET",
            SecretString::new("sk-ant-verysecret".into()),
        );
        let resolved = store.resolve(&realm, &placeholder).expect("must resolve");
        assert_eq!(resolved.expose_secret(), "sk-ant-verysecret");
    }

    #[test]
    fn resolve_returns_none_for_unknown_placeholder_never_guesses() {
        let store = store();
        let realm = RealmId::new("s1");
        let fake = Placeholder {
            category: "SECRET",
            id: crate::placeholder::placeholder_id(&[0u8; 32], "SECRET", "nope"),
        };
        assert!(store.resolve(&realm, &fake).is_none());
    }

    #[test]
    fn realms_are_mutually_invisible() {
        let store = store();
        let realm_a = RealmId::new("a");
        let realm_b = RealmId::new("b");
        let placeholder =
            store.get_or_create(&realm_a, "SECRET", SecretString::new("only-in-a".into()));
        assert!(store.resolve(&realm_b, &placeholder).is_none());
        assert!(store.resolve(&realm_a, &placeholder).is_some());
    }

    #[test]
    fn evict_zeroed_removes_realm_entirely() {
        let store = store();
        let realm = RealmId::new("s1");
        let placeholder = store.get_or_create(&realm, "SECRET", SecretString::new("bye".into()));
        store.evict_zeroed(&realm);
        assert!(store.resolve(&realm, &placeholder).is_none());
    }

    #[test]
    fn idle_ttl_expiry_makes_realm_unresolvable() {
        let config = MappingStoreConfig {
            realm_idle_ttl: Duration::from_millis(1),
            ..MappingStoreConfig::default()
        };
        let store = MemoryMappingStore::with_master_key([9u8; 32], config);
        let realm = RealmId::new("s1");
        let placeholder =
            store.get_or_create(&realm, "SECRET", SecretString::new("short-lived".into()));
        std::thread::sleep(Duration::from_millis(20));
        assert!(store.resolve(&realm, &placeholder).is_none());
    }

    #[test]
    fn different_master_key_cannot_decrypt_another_stores_ciphertext_shape() {
        // 平文が漏れないことの間接検証: 別マスター鍵の store では同じ realm/value でも
        // 異なる placeholder id になる (pepper が変わるため)。
        let store_a = MemoryMappingStore::with_master_key([1u8; 32], MappingStoreConfig::default());
        let store_b = MemoryMappingStore::with_master_key([2u8; 32], MappingStoreConfig::default());
        let realm = RealmId::new("s1");
        let pa = store_a.get_or_create(&realm, "SECRET", SecretString::new("same-value".into()));
        let pb = store_b.get_or_create(&realm, "SECRET", SecretString::new("same-value".into()));
        assert_ne!(pa.id, pb.id);
    }
}
