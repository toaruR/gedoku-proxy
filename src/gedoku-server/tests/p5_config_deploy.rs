//! P5 (設定/デプロイ) の DoD を検証する統合テスト群。
//! 計画 §5.5 の P5-T1/T2/T6/T9 に対応する。

use bytes::Bytes;
use gedoku_core::{AuthMode, Config, Mode, RealmStrategy, UnknownPathPolicy};
use gedoku_mock_upstream::{MockUpstream, ResponseSpec};
use http::StatusCode;
use std::sync::Arc;
use std::time::Duration;

fn base_config(upstream_base_url: String, realm_strategy: RealmStrategy, mode: Mode) -> Config {
    Config {
        listen_addr: "127.0.0.1:0".to_string(),
        upstream_base_url,
        auth_mode: AuthMode::Passthrough,
        upstream_auth_token: None,
        upstream_api_key: None,
        extra_upstream_headers: vec![],
        upstream_accept_encoding: "identity".to_string(),
        max_request_bytes: 32 * 1024 * 1024,
        models_passthrough: false,
        unknown_path_policy: UnknownPathPolicy::Passthrough,
        upstream_connect_timeout: Duration::from_secs(5),
        on_detect_error_fail_open: false,
        realm_strategy,
        mode,
        debug_dump_replacements: false,
    }
}

async fn spawn_proxy(config: Config) -> (String, tokio::task::JoinHandle<()>) {
    let router = gedoku_server::build_router(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{addr}"), handle)
}

/// 計画 P5-T1: `configs/gedoku.example.toml` は常に有効な設定として読み込める
/// (実際のロード経路 `Config::from_toml_file` 経由、gedoku-core 側のパーサー単体
/// テストとは別にリポジトリ内の相対パスから読める状態であることまで確認する)。
#[test]
fn example_config_file_loads_via_from_toml_file() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../configs/gedoku.example.toml"
    );
    let cfg = Config::from_toml_file(path).expect("example config must load");
    assert_eq!(cfg.realm_strategy, RealmStrategy::Credential);
    assert_eq!(cfg.mode, Mode::Enforce);
}

/// 計画 P5-T6 DoD: 観測モード ON 時、検出イベントはログに出るが本文は無変更で転送される。
#[tokio::test]
async fn observe_mode_forwards_secret_unmodified_to_upstream() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(
        mock.base_url(),
        RealmStrategy::Credential,
        Mode::Observe,
    ))
    .await;

    let secret = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcd";
    let request_body = serde_json::json!({
        "model": "claude-x",
        "max_tokens": 16,
        "messages": [{"role": "user", "content": format!("key: {secret}")}]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&request_body).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let recorded = mock.last_request().unwrap();
    let upstream_body = String::from_utf8(recorded.body.to_vec()).unwrap();
    assert!(
        upstream_body.contains(secret),
        "observe mode must forward the body byte-identical (unredacted)"
    );
    assert!(!upstream_body.contains("\u{27E6}GDK:"));

    mock.shutdown().await;
}

/// 計画 P5-T2 DoD (Global戦略): 異なるクライアント資格情報でも同一 Realm に
/// 集約され、同一原文には同一プレースホルダーが割り当てられる。
#[tokio::test]
async fn global_realm_strategy_shares_placeholder_across_different_credentials() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(
        mock.base_url(),
        RealmStrategy::Global,
        Mode::Enforce,
    ))
    .await;

    let secret = "leak@corp-secret.io";
    let client = reqwest::Client::new();
    let mut placeholders = Vec::new();

    for cred in ["key-client-a", "key-client-b"] {
        let request_body = serde_json::json!({
            "model": "claude-x",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": format!("email me at {secret}")}]
        });
        let resp = client
            .post(format!("{proxy_url}/v1/messages"))
            .header("content-type", "application/json")
            .header("x-api-key", cred)
            .body(serde_json::to_vec(&request_body).unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let recorded = mock.last_request().unwrap();
        let upstream_body = String::from_utf8(recorded.body.to_vec()).unwrap();
        assert!(!upstream_body.contains(secret));
        let after = upstream_body.split('\u{27E6}').nth(1).unwrap().to_string();
        placeholders.push(after.split('\u{27E7}').next().unwrap().to_string());
    }

    assert_eq!(
        placeholders[0], placeholders[1],
        "Global realm strategy must map the same secret to the same placeholder across clients"
    );

    mock.shutdown().await;
}

/// 計画 P5-T2 DoD (Ephemeral戦略): 同一クライアント・同一原文でも呼び出し毎に
/// マッピングが再利用されず、異なるプレースホルダーになる。
#[tokio::test]
async fn ephemeral_realm_strategy_never_reuses_mapping() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(
        mock.base_url(),
        RealmStrategy::Ephemeral,
        Mode::Enforce,
    ))
    .await;

    let secret = "leak@corp-secret.io";
    let client = reqwest::Client::new();
    let mut placeholders = Vec::new();

    for _ in 0..2 {
        let request_body = serde_json::json!({
            "model": "claude-x",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": format!("email me at {secret}")}]
        });
        let resp = client
            .post(format!("{proxy_url}/v1/messages"))
            .header("content-type", "application/json")
            .header("x-api-key", "same-client")
            .body(serde_json::to_vec(&request_body).unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let recorded = mock.last_request().unwrap();
        let upstream_body = String::from_utf8(recorded.body.to_vec()).unwrap();
        let after = upstream_body.split('\u{27E6}').nth(1).unwrap().to_string();
        placeholders.push(after.split('\u{27E7}').next().unwrap().to_string());
    }

    assert_ne!(placeholders[0], placeholders[1]);

    mock.shutdown().await;
}
