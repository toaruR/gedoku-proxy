//! プレースホルダーの命名規則と決定論的 ID 生成 (設計 §7.7, §7.8)。

use gedoku_core::RealmId;
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;

type HmacSha256 = Hmac<Sha256>;

const CROCKFORD_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaceholderId(String);

impl PlaceholderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// レスポンスストリームから走査した生の ID 文字列から再構築する (計画 P3)。
    /// `resolve` は ID のみをキーに引くため (§10.5 `RealmState::entries` 参照)、
    /// 文字集合の妥当性検証は呼び出し側 (`gedoku-stream::TailHoldScanner`) の責務とし、
    /// ここでは検証しない。未知 ID は単に `resolve` が `None` を返すだけで安全。
    pub fn from_scanned(raw: String) -> Self {
        PlaceholderId(raw)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    pub category: &'static str,
    pub id: PlaceholderId,
}

impl Placeholder {
    /// `⟦GDK:<CATEGORY>:<ID>⟧`。`GEDOKU_SENTINEL_ASCII=1` 相当の環境向けに
    /// `<<GDK:...>>` 形式も提供する (§7.7)。
    pub fn format(&self, ascii_fallback: bool) -> String {
        if ascii_fallback {
            format!("<<GDK:{}:{}>>", self.category, self.id.0)
        } else {
            format!("\u{27E6}GDK:{}:{}\u{27E7}", self.category, self.id.0)
        }
    }
}

/// `realm.pepper = HKDF(master_key, salt = realm_id)` (§7.8)。プロセス内・非永続。
pub fn derive_realm_pepper(master_key: &[u8], realm: &RealmId) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(realm.as_str().as_bytes()), master_key);
    let mut okm = [0u8; 32];
    hk.expand(b"gedoku-mapstore:placeholder-pepper:v1", &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

/// 保存時暗号化鍵。pepper と HKDF info を分け、用途間の鍵を分離する (pepper の
/// 漏洩から暗号鍵を、暗号鍵の漏洩から pepper を、それぞれ独立させるため)。
pub fn derive_enc_key(master_key: &[u8], realm: &RealmId) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(realm.as_str().as_bytes()), master_key);
    let mut okm = [0u8; 32];
    hk.expand(b"gedoku-mapstore:entry-encryption-key:v1", &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

/// §7.8 `normalize()`。カテゴリ文字列は gedoku-detect の category 定数と同じ
/// 文字列を渡す想定 (循環依存を避けるため型ではなく `&str` で受け取る)。
pub fn normalize(category: &str, value: &str) -> String {
    let nfkc: String = value.nfkc().collect();
    match category {
        "PII_EMAIL" => nfkc.to_lowercase(),
        "PII_PHONE" => nfkc
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '+')
            .collect(),
        "PII_CARD" => nfkc
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect(),
        // 姓名間の空白(半角/全角/複数)を単一の半角スペースへ統一する (設計§7.10.10)。
        "PII_NAME_JA" => nfkc.split_whitespace().collect::<Vec<_>>().join(" "),
        // PII_ADDRESS_JA を含むそれ以外: NFKCのみ。丁目/番地表記ゆれ(4丁目 vs 4-)は
        // 別プレースホルダーのまま区別する (設計§7.10.10)。
        _ => nfkc,
    }
}

/// `ID(value, realm) = Base32Crockford(HMAC-SHA256(pepper, normalize(value)))[0:10]` (§7.8)。
pub fn placeholder_id(pepper: &[u8; 32], category: &str, normalized_value: &str) -> PlaceholderId {
    let mut mac = HmacSha256::new_from_slice(pepper).expect("HMAC accepts any key length");
    mac.update(category.as_bytes());
    mac.update(b":");
    mac.update(normalized_value.as_bytes());
    let digest = mac.finalize().into_bytes();
    PlaceholderId(crockford_encode_50bits(&digest))
}

/// HMAC-SHA256 ダイジェストの先頭56ビットから、上位50ビットを Crockford Base32
/// 10文字にエンコードする (§7.7: 50 bit)。
fn crockford_encode_50bits(digest: &[u8]) -> String {
    let mut val: u64 = 0;
    for b in &digest[0..7] {
        val = (val << 8) | (*b as u64);
    }
    val >>= 6; // 56 bit -> 上位50bitのみ残す
    let mut out = [0u8; 10];
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = 5 * (9 - i);
        let idx = ((val >> shift) & 0b1_1111) as usize;
        *slot = CROCKFORD_ALPHABET[idx];
    }
    String::from_utf8(out.to_vec()).expect("crockford alphabet is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_realm_and_value_produce_same_id() {
        let master_key = [7u8; 32];
        let realm = RealmId::new("session-abc");
        let pepper = derive_realm_pepper(&master_key, &realm);
        let a = placeholder_id(
            &pepper,
            "PII_EMAIL",
            &normalize("PII_EMAIL", "A@Example.com"),
        );
        let b = placeholder_id(
            &pepper,
            "PII_EMAIL",
            &normalize("PII_EMAIL", "a@example.com"),
        );
        assert_eq!(
            a, b,
            "normalize は大文字小文字を吸収するので同一IDになるべき"
        );
    }

    #[test]
    fn different_realm_produces_different_id() {
        let master_key = [7u8; 32];
        let realm_a = RealmId::new("session-a");
        let realm_b = RealmId::new("session-b");
        let value = normalize("SECRET", "sk-ant-abc123");
        let id_a = placeholder_id(
            &derive_realm_pepper(&master_key, &realm_a),
            "SECRET",
            &value,
        );
        let id_b = placeholder_id(
            &derive_realm_pepper(&master_key, &realm_b),
            "SECRET",
            &value,
        );
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn id_is_exactly_10_crockford_chars() {
        let master_key = [1u8; 32];
        let realm = RealmId::new("r");
        let pepper = derive_realm_pepper(&master_key, &realm);
        let id = placeholder_id(&pepper, "SECRET", "value");
        assert_eq!(id.as_str().len(), 10);
        assert!(id.as_str().bytes().all(|b| CROCKFORD_ALPHABET.contains(&b)));
    }

    #[test]
    fn secret_category_is_not_normalized_beyond_nfkc() {
        assert_ne!(
            normalize("SECRET", "Sk-Ant-ABC"),
            normalize("SECRET", "sk-ant-abc")
        );
    }

    #[test]
    fn normalize_ja_name_collapses_whitespace() {
        assert_eq!(
            normalize("PII_NAME_JA", "山田\u{3000}太郎"),
            normalize("PII_NAME_JA", "山田 太郎"),
            "全角/半角スペースは同一プレースホルダーIDになるべき"
        );
        assert_eq!(
            normalize("PII_NAME_JA", "山田   太郎"),
            normalize("PII_NAME_JA", "山田 太郎"),
            "連続スペースは単一スペースに畳まれるべき"
        );
    }

    #[test]
    fn normalize_ja_address_preserves_banchi_notation_difference() {
        assert_ne!(
            normalize("PII_ADDRESS_JA", "港区芝公園4丁目"),
            normalize("PII_ADDRESS_JA", "港区芝公園4-"),
            "丁目/ハイフン表記ゆれはNFKCのみでは同一視されないべき"
        );
    }

    #[test]
    fn placeholder_format_uses_sentinel_brackets_by_default() {
        let p = Placeholder {
            category: "PII_EMAIL",
            id: PlaceholderId("5QK2ZP31XY".to_string()),
        };
        assert_eq!(p.format(false), "\u{27E6}GDK:PII_EMAIL:5QK2ZP31XY\u{27E7}");
        assert_eq!(p.format(true), "<<GDK:PII_EMAIL:5QK2ZP31XY>>");
    }
}
