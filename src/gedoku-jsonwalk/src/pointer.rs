//! JSON Pointer (RFC 6901 風) の組み立て (設計 §7.3)。

/// 親ポインタにキー (オブジェクトフィールド名) を追加する。
pub fn push_key(parent: &str, key: &str) -> String {
    format!("{parent}/{}", escape(key))
}

/// 親ポインタに配列インデックスを追加する。
pub fn push_index(parent: &str, index: usize) -> String {
    format!("{parent}/{index}")
}

/// RFC 6901: `~` -> `~0`, `/` -> `~1`。
fn escape(key: &str) -> String {
    if key.contains('~') || key.contains('/') {
        key.replace('~', "~0").replace('/', "~1")
    } else {
        key.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_nested_pointer() {
        let p = push_key("", "messages");
        let p = push_index(&p, 0);
        let p = push_key(&p, "content");
        assert_eq!(p, "/messages/0/content");
    }

    #[test]
    fn escapes_special_chars_in_keys() {
        assert_eq!(push_key("", "a/b~c"), "/a~1b~0c");
    }
}
