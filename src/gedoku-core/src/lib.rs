//! gedoku-core: 設定・エラー型・共通定義 (設計 §3-5, §13)

use serde::Serialize;
use std::env;
use std::time::Duration;

/// アップストリームへの資格情報転送モード (設計 §5.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// クライアントの Authorization / x-api-key をそのまま転送 (既定)
    Passthrough,
    /// プロキシ自身の資格情報に差し替える
    Replace,
    /// クライアント資格情報を転送しつつ追加ヘッダーを付与する
    Augment,
}

impl AuthMode {
    fn from_env_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "replace" => AuthMode::Replace,
            "augment" => AuthMode::Augment,
            _ => AuthMode::Passthrough,
        }
    }
}

/// Realm 識別子 (設計 §10.4)。相互に不可視なマッピングストアのスコープ単位。
/// Realm の導出戦略 (session/credential/global/ephemeral) は P5-T2 で実装する。
/// P2 時点では呼び出し側 (gedoku-server) が構築済みの ID を渡す。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RealmId(pub String);

impl RealmId {
    pub fn new(id: impl Into<String>) -> Self {
        RealmId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 未知パスの扱い (設計 §3.1 #9)。v1 では passthrough のみサポート。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownPathPolicy {
    Passthrough,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_base_url: String,
    pub auth_mode: AuthMode,
    pub upstream_auth_token: Option<String>,
    pub upstream_api_key: Option<String>,
    pub extra_upstream_headers: Vec<(String, String)>,
    pub upstream_accept_encoding: String,
    pub max_request_bytes: usize,
    pub models_passthrough: bool,
    pub unknown_path_policy: UnknownPathPolicy,
    pub upstream_connect_timeout: Duration,
    /// ADR-9: 検出エンジン障害 (JSON parse 失敗含む) 時の既定挙動。
    /// `true` (fail_open) なら未加工のまま転送、既定 `false` (fail_closed) は拒否する。
    /// サイズ超過 (`max_request_bytes`) は本設定によらず常に fail-open (ADR-9 非対称性)。
    pub on_detect_error_fail_open: bool,
}

impl Config {
    /// 環境変数から設定を読み込む (設計 §3-5 の各既定値に準拠)。
    pub fn from_env() -> Self {
        Config {
            listen_addr: env::var("GEDOKU_LISTEN_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:18787".to_string()),
            upstream_base_url: env::var("GEDOKU_UPSTREAM_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
            auth_mode: env::var("GEDOKU_UPSTREAM_AUTH_MODE")
                .map(|s| AuthMode::from_env_str(&s))
                .unwrap_or(AuthMode::Passthrough),
            upstream_auth_token: env::var("GEDOKU_UPSTREAM_AUTH_TOKEN").ok(),
            upstream_api_key: env::var("GEDOKU_UPSTREAM_API_KEY").ok(),
            extra_upstream_headers: parse_header_list(
                env::var("GEDOKU_EXTRA_UPSTREAM_HEADERS")
                    .unwrap_or_default()
                    .as_str(),
            ),
            upstream_accept_encoding: env::var("GEDOKU_UPSTREAM_ACCEPT_ENCODING")
                .unwrap_or_else(|_| "identity".to_string()),
            max_request_bytes: env::var("GEDOKU_MAX_REQUEST_BYTES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(32 * 1024 * 1024),
            models_passthrough: env::var("GEDOKU_MODELS_PASSTHROUGH")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            unknown_path_policy: UnknownPathPolicy::Passthrough,
            upstream_connect_timeout: Duration::from_secs(
                env::var("GEDOKU_UPSTREAM_CONNECT_TIMEOUT_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30),
            ),
            on_detect_error_fail_open: env::var("GEDOKU_ON_DETECT_ERROR")
                .map(|v| v.eq_ignore_ascii_case("fail_open"))
                .unwrap_or(false),
        }
    }
}

fn parse_header_list(raw: &str) -> Vec<(String, String)> {
    raw.split(',')
        .filter_map(|pair| {
            let (name, value) = pair.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() {
                None
            } else {
                Some((name.to_string(), value.to_string()))
            }
        })
        .collect()
}

/// Anthropic 形式のエラー本文 (設計 §13.3)。gedoku-proxy 自身が原因のエラーでのみ生成する。
#[derive(Debug, Serialize)]
pub struct AnthropicError {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub error: AnthropicErrorBody,
}

#[derive(Debug, Serialize)]
pub struct AnthropicErrorBody {
    #[serde(rename = "type")]
    pub error_type: &'static str,
    pub message: String,
}

impl AnthropicError {
    /// message は "gedoku-proxy: " で始める (E7: capability_rejected: を絶対に含めない)。
    pub fn new(error_type: &'static str, message: impl Into<String>) -> Self {
        AnthropicError {
            kind: "error",
            error: AnthropicErrorBody {
                error_type,
                message: format!("gedoku-proxy: {}", message.into()),
            },
        }
    }

    pub fn authentication_error(message: impl Into<String>) -> Self {
        Self::new("authentication_error", message)
    }

    pub fn invalid_request_error(message: impl Into<String>) -> Self {
        Self::new("invalid_request_error", message)
    }

    pub fn api_error(message: impl Into<String>) -> Self {
        Self::new("api_error", message)
    }

    pub fn rate_limit_error(message: impl Into<String>) -> Self {
        Self::new("rate_limit_error", message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_list() {
        let parsed = parse_header_list("X-Org-Route: foo, X-Tenant:bar");
        assert_eq!(
            parsed,
            vec![
                ("X-Org-Route".to_string(), "foo".to_string()),
                ("X-Tenant".to_string(), "bar".to_string()),
            ]
        );
    }

    #[test]
    fn error_message_never_contains_capability_rejected() {
        let e = AnthropicError::api_error("upstream connect timeout");
        assert!(!e.error.message.contains("capability_rejected:"));
        assert!(e.error.message.starts_with("gedoku-proxy: "));
    }
}
