//! プレースホルダー文法の定数 (設計 §7.7, gedoku-mapstore::placeholder と対応)。
//!
//! 形式: `⟦GDK:<CATEGORY>:<ID>⟧`。CATEGORY は `[A-Z][A-Z0-9_]{0,23}`、
//! ID は Crockford Base32 10 文字固定 (`gedoku_mapstore::placeholder` 参照)。
//! 非対応言語 (このクレート) からは `&'static str` の定数として複製する
//! (gedoku-mapstore への依存を避けるため。計画 §3 の「他クレートに非依存」方針に倣う)。

pub const OPEN: &[u8] = "\u{27E6}".as_bytes();
pub const CLOSE: &[u8] = "\u{27E7}".as_bytes();
pub const PREFIX: &[u8] = b"GDK:";
pub const MAX_CATEGORY_LEN: usize = 24;
pub const ID_LEN: usize = 10;

/// プレースホルダー1個の理論最大長 (OPEN+PREFIX+category(24)+':'+ID(10)+CLOSE)。
pub const MAX_PLACEHOLDER_LEN: usize =
    OPEN.len() + PREFIX.len() + MAX_CATEGORY_LEN + 1 + ID_LEN + CLOSE.len();

/// 設計 §12 の tail-hold バイト上限 (理論最大長より余裕を持たせた安全弁)。
pub const MAX_HOLD_BYTES: usize = 64;

pub fn is_category_start(b: u8) -> bool {
    b.is_ascii_uppercase()
}

pub fn is_category_char(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'
}

pub fn is_id_char(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}
