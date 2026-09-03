//! 秘密値のメモリ上表現 (設計 §10.3 M2/M6, §12 M系)。
//!
//! `Debug`/`Display` を意図的に実装しない (原文を含む型に Debug を実装しない、
//! という M 系要求の型システムによる強制)。Drop 時に `zeroize` でゼロ埋めする。

use zeroize::Zeroizing;

pub struct SecretString(Zeroizing<String>);

impl SecretString {
    pub fn new(value: String) -> Self {
        SecretString(Zeroizing::new(value))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl Clone for SecretString {
    fn clone(&self) -> Self {
        SecretString(Zeroizing::new((*self.0).clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_value() {
        let s = SecretString::new("hunter2".to_string());
        assert_eq!(s.expose_secret(), "hunter2");
    }
}
