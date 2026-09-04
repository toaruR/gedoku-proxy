//! gedoku-core: 設定・エラー型・共通定義 (設計 §3-5, §13)

use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;

/// `--config`/`GEDOKU_CONFIG_FILE` 未指定時に自動で読みに行く既定の設定ファイルパス。
const DEFAULT_CONFIG_PATH: &str = "configs/gedoku.toml";

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

/// Realm 導出戦略 (設計 §10.4, 計画 P5-T2, OQ-2)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealmStrategy {
    /// クライアント資格情報 (x-api-key/Authorization) のハッシュを Realm とする (既定、OQ-2)。
    Credential,
    /// 全リクエストを単一 Realm に集約する。
    Global,
    /// 呼び出し毎に新規 Realm を発行する (マッピングを一切再利用しない)。
    Ephemeral,
}

impl RealmStrategy {
    fn from_str_lenient(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "global" => RealmStrategy::Global,
            "ephemeral" => RealmStrategy::Ephemeral,
            _ => RealmStrategy::Credential,
        }
    }
}

/// 検出/置換の適用モード (設計 §19, GAP-7, OQ-3, 計画 P5-T6)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 検出のみ行い監査ログに出すが、本文は無変更で転送する。
    Observe,
    /// 検出した秘密をプレースホルダーへ置換する (既定、OQ-3: 本番影響ゼロを優先し既定off)。
    Enforce,
}

impl Mode {
    fn from_str_lenient(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "observe" => Mode::Observe,
            _ => Mode::Enforce,
        }
    }
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
    /// Realm 導出戦略 (計画 P5-T2)。
    pub realm_strategy: RealmStrategy,
    /// 検出/置換の適用モード (計画 P5-T6)。
    pub mode: Mode,
    /// デバッグ用途: 置換したプレースホルダー(原文は含まない)とカテゴリ、
    /// 前後数語の文脈を固定パスのテキストファイルへ追記する (既定 off)。
    pub debug_dump_replacements: bool,
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
            realm_strategy: env::var("GEDOKU_REALM_STRATEGY")
                .map(|s| RealmStrategy::from_str_lenient(&s))
                .unwrap_or(RealmStrategy::Credential),
            mode: env::var("GEDOKU_MODE")
                .map(|s| Mode::from_str_lenient(&s))
                .unwrap_or(Mode::Enforce),
            debug_dump_replacements: env::var("GEDOKU_DEBUG_DUMP_REPLACEMENTS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
        }
    }

    /// TOML 文字列から設定を読み込む (計画 P5-T1)。未知キー/型不一致は `Err` で返す
    /// (`#[serde(deny_unknown_fields)]`)。ファイルに記載しないフィールドは `from_env` と
    /// 同じ既定値を採用する (環境変数は参照しない、ファイル内容のみで完結させる)。
    pub fn from_toml_str(raw: &str) -> Result<Config, ConfigError> {
        let file: FileConfig = toml::from_str(raw)?;
        let defaults = Config::from_env_defaults_only();
        Ok(Config {
            listen_addr: file.listen_addr.unwrap_or(defaults.listen_addr),
            upstream_base_url: file.upstream_base_url.unwrap_or(defaults.upstream_base_url),
            auth_mode: file
                .auth_mode
                .as_deref()
                .map(AuthMode::from_env_str)
                .unwrap_or(defaults.auth_mode),
            upstream_auth_token: file.upstream_auth_token,
            upstream_api_key: file.upstream_api_key,
            extra_upstream_headers: file
                .extra_upstream_headers
                .into_iter()
                .map(|h| (h.name, h.value))
                .collect(),
            upstream_accept_encoding: file
                .upstream_accept_encoding
                .unwrap_or(defaults.upstream_accept_encoding),
            max_request_bytes: file.max_request_bytes.unwrap_or(defaults.max_request_bytes),
            models_passthrough: file
                .models_passthrough
                .unwrap_or(defaults.models_passthrough),
            unknown_path_policy: UnknownPathPolicy::Passthrough,
            upstream_connect_timeout: file
                .upstream_connect_timeout_secs
                .map(Duration::from_secs)
                .unwrap_or(defaults.upstream_connect_timeout),
            on_detect_error_fail_open: file
                .on_detect_error
                .as_deref()
                .map(|v| v.eq_ignore_ascii_case("fail_open"))
                .unwrap_or(defaults.on_detect_error_fail_open),
            realm_strategy: file
                .realm_strategy
                .as_deref()
                .map(RealmStrategy::from_str_lenient)
                .unwrap_or(defaults.realm_strategy),
            mode: file
                .mode
                .as_deref()
                .map(Mode::from_str_lenient)
                .unwrap_or(defaults.mode),
            debug_dump_replacements: file
                .debug_dump_replacements
                .unwrap_or(defaults.debug_dump_replacements),
        })
    }

    /// 設定ファイルのパスを引数に取るバージョン (計画 P5-T1)。
    pub fn from_toml_file(path: &str) -> Result<Config, ConfigError> {
        let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_string(),
            source,
        })?;
        Config::from_toml_str(&raw)
    }

    /// 設定ファイルの解決順は
    /// `--config`/`-c` 起動引数 > `GEDOKU_CONFIG_FILE` 環境変数 >
    /// 既定パス `configs/gedoku.toml` (存在すれば) > 環境変数群 (計画 P5-T1)。
    /// 不正な設定ファイルは起動を失敗させる (`panic`)。
    pub fn load() -> Config {
        Self::load_from_args(env::args().skip(1))
    }

    /// [`Config::load`] の本体。CLI引数をテストから注入できるよう分離している。
    pub fn load_from_args<I: IntoIterator<Item = String>>(args: I) -> Config {
        let path = Self::config_path_from_args(args)
            .or_else(|| env::var("GEDOKU_CONFIG_FILE").ok())
            .or_else(|| {
                let default = DEFAULT_CONFIG_PATH;
                std::path::Path::new(default)
                    .is_file()
                    .then(|| default.to_string())
            });
        match path {
            Some(path) => Config::from_toml_file(&path)
                .unwrap_or_else(|e| panic!("gedoku-proxy: invalid config file: {e}")),
            None => Config::from_env(),
        }
    }

    /// `--config <path>` / `--config=<path>` / `-c <path>` を探す。
    fn config_path_from_args<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            if arg == "--config" || arg == "-c" {
                return iter.next();
            }
            if let Some(value) = arg.strip_prefix("--config=") {
                return Some(value.to_string());
            }
        }
        None
    }

    /// `from_toml_str` が「ファイル未指定フィールドの既定値」として使う。環境変数は
    /// 参照しないよう `from_env` から意図的に切り離す (テスト時の環境汚染を避けるため)。
    fn from_env_defaults_only() -> Config {
        Config {
            listen_addr: "127.0.0.1:18787".to_string(),
            upstream_base_url: "https://api.anthropic.com".to_string(),
            auth_mode: AuthMode::Passthrough,
            upstream_auth_token: None,
            upstream_api_key: None,
            extra_upstream_headers: Vec::new(),
            upstream_accept_encoding: "identity".to_string(),
            max_request_bytes: 32 * 1024 * 1024,
            models_passthrough: false,
            unknown_path_policy: UnknownPathPolicy::Passthrough,
            upstream_connect_timeout: Duration::from_secs(30),
            on_detect_error_fail_open: false,
            realm_strategy: RealmStrategy::Credential,
            mode: Mode::Enforce,
            debug_dump_replacements: false,
        }
    }
}

/// TOML から直接デシリアライズする素の設定形。未知キーはエラーにする (P5-T1 DoD)。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileConfig {
    listen_addr: Option<String>,
    upstream_base_url: Option<String>,
    auth_mode: Option<String>,
    upstream_auth_token: Option<String>,
    upstream_api_key: Option<String>,
    extra_upstream_headers: Vec<FileHeaderPair>,
    upstream_accept_encoding: Option<String>,
    max_request_bytes: Option<usize>,
    models_passthrough: Option<bool>,
    upstream_connect_timeout_secs: Option<u64>,
    on_detect_error: Option<String>,
    realm_strategy: Option<String>,
    mode: Option<String>,
    debug_dump_replacements: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileHeaderPair {
    name: String,
    value: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),
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

    #[test]
    fn toml_config_overrides_only_specified_fields() {
        let raw = r#"
            listen_addr = "0.0.0.0:9000"
            mode = "observe"
            realm_strategy = "global"
        "#;
        let cfg = Config::from_toml_str(raw).expect("valid toml");
        assert_eq!(cfg.listen_addr, "0.0.0.0:9000");
        assert_eq!(cfg.mode, Mode::Observe);
        assert_eq!(cfg.realm_strategy, RealmStrategy::Global);
        // 未指定フィールドは既定値のまま。
        assert_eq!(cfg.upstream_base_url, "https://api.anthropic.com");
        assert_eq!(cfg.auth_mode, AuthMode::Passthrough);
    }

    #[test]
    fn toml_config_rejects_unknown_keys() {
        let raw = r#"typo_field = "oops""#;
        let err = Config::from_toml_str(raw).expect_err("must reject unknown key");
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn toml_config_rejects_type_mismatch() {
        let raw = r#"max_request_bytes = "not-a-number""#;
        let err = Config::from_toml_str(raw).expect_err("must reject type mismatch");
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn toml_config_parses_extra_upstream_headers_table() {
        let raw = r#"
            [[extra_upstream_headers]]
            name = "x-org-route"
            value = "team-a"
        "#;
        let cfg = Config::from_toml_str(raw).expect("valid toml");
        assert_eq!(
            cfg.extra_upstream_headers,
            vec![("x-org-route".to_string(), "team-a".to_string())]
        );
    }

    #[test]
    fn from_toml_file_reports_io_error_for_missing_file() {
        let err = Config::from_toml_file("does/not/exist.toml").expect_err("missing file");
        assert!(matches!(err, ConfigError::Io { .. }));
    }

    /// `configs/gedoku.example.toml` (計画 P5-T1) が常にパース可能であることを保証し、
    /// ドキュメントと実装の乖離を検出する。
    #[test]
    fn example_config_file_parses_successfully() {
        let raw = include_str!("../../../configs/gedoku.example.toml");
        let cfg = Config::from_toml_str(raw).expect("example config must be valid");
        assert_eq!(cfg.listen_addr, "0.0.0.0:18787");
        assert_eq!(cfg.realm_strategy, RealmStrategy::Credential);
        assert_eq!(cfg.mode, Mode::Enforce);
    }

    #[test]
    fn config_path_from_args_supports_space_and_equals_forms() {
        fn args(v: &[&str]) -> Vec<String> {
            v.iter().map(|s| s.to_string()).collect()
        }
        assert_eq!(
            Config::config_path_from_args(args(&["--config", "foo.toml"])),
            Some("foo.toml".to_string())
        );
        assert_eq!(
            Config::config_path_from_args(args(&["--config=foo.toml"])),
            Some("foo.toml".to_string())
        );
        assert_eq!(
            Config::config_path_from_args(args(&["-c", "foo.toml"])),
            Some("foo.toml".to_string())
        );
        assert_eq!(Config::config_path_from_args(args(&["--other"])), None);
        assert_eq!(Config::config_path_from_args(args(&[])), None);
    }

    /// CLI引数で指定した設定ファイルが最優先で読まれる (環境変数/既定パスより優先)。
    #[test]
    fn load_from_args_prefers_cli_config_over_default() {
        let dir = std::env::temp_dir().join(format!(
            "gedoku-core-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cli.toml");
        std::fs::write(&path, r#"listen_addr = "0.0.0.0:9999""#).unwrap();

        let cfg = Config::load_from_args(vec![
            "--config".to_string(),
            path.to_str().unwrap().to_string(),
        ]);
        assert_eq!(cfg.listen_addr, "0.0.0.0:9999");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
