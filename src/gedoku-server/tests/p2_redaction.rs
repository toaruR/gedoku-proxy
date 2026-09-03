//! P2 (検出/置換) の DoD を検証する統合テスト群。
//! 計画 §5.2 の P2-T7/P2-T8 に対応する。

use bytes::Bytes;
use gedoku_core::{AuthMode, Config, Mode, RealmStrategy, UnknownPathPolicy};
use gedoku_mock_upstream::{MockUpstream, ResponseSpec};
use http::StatusCode;
use std::sync::Arc;
use std::time::Duration;

fn base_config(upstream_base_url: String) -> Config {
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
        realm_strategy: RealmStrategy::Credential,
        mode: Mode::Enforce,
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

/// P2-T7 DoD: `/v1/messages` に埋め込まれた秘密情報が、アップストリームへ届く
/// ボディには一切現れず、プレースホルダーへ置換された状態で届くこと。
#[tokio::test]
async fn secrets_never_reach_upstream_body() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(
        br#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"ok"}]}"#,
    )))
    .await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let secret = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcd";
    let request_body = serde_json::json!({
        "model": "claude-x",
        "max_tokens": 16,
        "messages": [{
            "role": "user",
            "content": format!("here is my key: {secret}, please store it")
        }]
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
        !upstream_body.contains(secret),
        "upstream body must contain zero occurrences of the original secret"
    );
    assert!(
        upstream_body.contains("\u{27E6}GDK:SECRET:"),
        "upstream body must contain the redaction placeholder sentinel"
    );

    // 構造 (JSON の形) 自体は保たれていること。
    let parsed: serde_json::Value = serde_json::from_str(&upstream_body).unwrap();
    assert_eq!(parsed["model"], "claude-x");
    assert_eq!(parsed["max_tokens"], 16);

    mock.shutdown().await;
}

/// P2-T8 (縮小版完了ハーネス): 計画が示す 300 件規模の合成コーパスは用意せず、
/// 主要カテゴリ (SECRET/PII_EMAIL/PII_PHONE/PII_CARD) を代表する小規模コーパスで
/// 「原文が upstream に一切残らない」ことのみを確認する。大規模 F1 測定は
/// gedoku-detect クレート側の単体テストが既に担っている (P1 の HAR-replay 省略と同じ方針)。
#[tokio::test]
async fn small_corpus_leaves_no_original_secret_in_upstream_body() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let corpus = [
        "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcd",
        "contact me at jane.doe@example-corp.internal for details",
        "AKIAABCDEFGHIJKLMNOP is our old access key, rotate it",
        "card on file: 4111 1111 1111 1111",
    ];

    let client = reqwest::Client::new();
    for secret in corpus {
        let request_body = serde_json::json!({
            "model": "claude-x",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": secret}]
        });
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
            !upstream_body.contains(secret),
            "leaked secret into upstream body: {secret}"
        );
    }

    mock.shutdown().await;
}
