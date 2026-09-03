//! P0 (透過プロキシ骨格) の DoD を検証する統合テスト群。
//! 計画 §5.0 の P0-T1〜T9 に対応する。

use bytes::Bytes;
use gedoku_core::{AuthMode, Config, Mode, RealmStrategy, UnknownPathPolicy};
use gedoku_mock_upstream::{MockUpstream, ResponseSpec};
use http::StatusCode;
use sha2::{Digest, Sha256};
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

/// gedoku-server を実プロセスではなく in-process axum::serve として起動するテストヘルパー。
async fn spawn_proxy(config: Config) -> (String, tokio::task::JoinHandle<()>) {
    let router = gedoku_server::build_router(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{addr}"), handle)
}

/// P0-T1: /health が 200 を返す。
#[tokio::test]
async fn health_returns_200() {
    let (proxy_url, _h) = spawn_proxy(base_config("http://127.0.0.1:1".to_string())).await;
    let resp = reqwest::get(format!("{proxy_url}/health")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// P0-T2: 非stream `/v1/messages` がリクエストを1バイトも改変せず転送し、
/// レスポンスbodyのSHA256がupstream直叩き時と一致する。
#[tokio::test]
async fn messages_non_stream_byte_identical() {
    let response_body = Bytes::from_static(
        br#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"hello"}]}"#,
    );
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(response_body.clone())).await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let request_body =
        br#"{"model":"claude-x","max_tokens":16,"messages":[{"role":"user","content":"hi"}]}"#;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("content-type", "application/json")
        .body(request_body.to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let received_body = resp.bytes().await.unwrap();

    // upstream 直叩き時の期待 SHA256 と一致することを確認 (DoD の "SHA256が一致" 相当)
    let mut expected_hasher = Sha256::new();
    expected_hasher.update(&response_body);
    let expected_sha = hex::encode(expected_hasher.finalize());

    let mut actual_hasher = Sha256::new();
    actual_hasher.update(&received_body);
    let actual_sha = hex::encode(actual_hasher.finalize());

    assert_eq!(actual_sha, expected_sha);
    assert_eq!(received_body.as_ref(), response_body.as_ref());

    // upstream が受け取ったリクエストボディも 1 バイトも改変されていないこと。
    let recorded = mock.last_request().unwrap();
    assert_eq!(recorded.body.as_ref(), request_body);

    mock.shutdown().await;
}

/// P0-T3: SSEストリーミング応答を、フレーム数・各フレームの生バイト列が完全一致した状態で転送する。
#[tokio::test]
async fn sse_frames_pass_through_unmodified() {
    let frames: Vec<Bytes> = vec![
        Bytes::from_static(b"event: message_start\ndata: {\"type\":\"message_start\"}\n\n"),
        Bytes::from_static(b": ping\n\n"),
        Bytes::from_static(b"event: content_block_delta\ndata: {\"delta\":{\"text\":\"hi\"}}\n\n"),
        Bytes::from_static(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"),
    ];

    let mock = MockUpstream::spawn(ResponseSpec::Chunked {
        status: StatusCode::OK,
        headers: vec![("content-type".into(), "text/event-stream".into())],
        chunks: frames.clone(),
        delay_between: Duration::from_millis(5),
    })
    .await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages?beta=true"))
        .header("content-type", "application/json")
        .body(r#"{"stream":true}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    // クライアント側で受信した生バイト列を、送出チャンクごとに記録する。
    use futures_util::StreamExt;
    let mut received_chunks: Vec<Bytes> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        received_chunks.push(chunk.unwrap());
    }

    // フレーム数・生バイト列が完全一致 (プロキシがチャンクを結合/分割していないこと)。
    assert_eq!(received_chunks.len(), frames.len());
    for (received, expected) in received_chunks.iter().zip(frames.iter()) {
        assert_eq!(received.as_ref(), expected.as_ref());
    }

    // 連結した場合も完全に一致すること (最終防衛線)。
    let received_concat: Vec<u8> = received_chunks.iter().flat_map(|b| b.to_vec()).collect();
    let expected_concat: Vec<u8> = frames.iter().flat_map(|b| b.to_vec()).collect();
    assert_eq!(received_concat, expected_concat);

    mock.shutdown().await;
}

/// P0-T4: 認証3方式 (Authorization / x-api-key / カスタムヘッダー) がバイト一致で転送される。
#[tokio::test]
async fn auth_headers_forwarded_byte_identical() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("authorization", "Bearer sk-client-token-abc123")
        .header("x-api-key", "client-api-key-xyz789")
        .header("x-org-route", "tenant-42") // ANTHROPIC_CUSTOM_HEADERS 由来のカスタムヘッダー相当
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let recorded = mock.last_request().unwrap();
    assert_eq!(
        recorded.header("authorization"),
        Some("Bearer sk-client-token-abc123")
    );
    assert_eq!(recorded.header("x-api-key"), Some("client-api-key-xyz789"));
    assert_eq!(recorded.header("x-org-route"), Some("tenant-42"));

    mock.shutdown().await;
}

/// P0-T5: 未知の (未定義) リクエストヘッダーがオープンリスト方針で転送される。
#[tokio::test]
async fn unknown_request_header_forwarded() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-gedoku-test-unknown-header", "hello")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let recorded = mock.last_request().unwrap();
    assert_eq!(
        recorded.header("x-gedoku-test-unknown-header"),
        Some("hello")
    );

    mock.shutdown().await;
}

/// P0-T6: upstream が返す未知のレスポンスヘッダーがクライアントに到達する (GAP-6反映)。
#[tokio::test]
async fn unknown_response_header_forwarded() {
    let mock = MockUpstream::spawn(ResponseSpec::Fixed {
        status: StatusCode::OK,
        headers: vec![
            ("content-type".into(), "application/json".into()),
            (
                "x-gedoku-test-unknown-response-header".into(),
                "world".into(),
            ),
            ("anthropic-request-id".into(), "req_123".into()),
        ],
        body: Bytes::from_static(b"{}"),
    })
    .await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let resp = reqwest::Client::new()
        .post(format!("{proxy_url}/v1/messages"))
        .body("{}")
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.headers()
            .get("x-gedoku-test-unknown-response-header")
            .unwrap(),
        "world"
    );
    assert_eq!(
        resp.headers().get("anthropic-request-id").unwrap(),
        "req_123"
    );

    mock.shutdown().await;
}

/// P0-T7: upstream が返す 4xx/5xx の status/body/header が1バイトも変更されずクライアントに到達する。
#[tokio::test]
async fn error_status_body_headers_pass_through_unmodified() {
    let error_body = Bytes::from_static(
        br#"{"type":"error","error":{"type":"invalid_request_error","message":"thinking field rejected"}}"#,
    );
    let mock = MockUpstream::spawn(ResponseSpec::Fixed {
        status: StatusCode::BAD_REQUEST,
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("request-id".into(), "req_err_1".into()),
        ],
        body: error_body.clone(),
    })
    .await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let resp = reqwest::Client::new()
        .post(format!("{proxy_url}/v1/messages"))
        .body("{}")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(resp.headers().get("request-id").unwrap(), "req_err_1");
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), error_body.as_ref());

    mock.shutdown().await;
}

/// P0-T8: `/v1/models` (既定OFFなら404、有効化時はupstream応答をそのまま返す)、
/// `count_tokens` (v1では常に404) の透過のみ挙動を確認する。
#[tokio::test]
async fn models_disabled_by_default_returns_404() {
    let mock =
        MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{\"data\":[]}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let resp = reqwest::get(format!("{proxy_url}/v1/models?limit=1000"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    // upstream には到達していない。
    assert!(mock.requests().is_empty());

    mock.shutdown().await;
}

#[tokio::test]
async fn models_passthrough_when_enabled() {
    let body = Bytes::from_static(b"{\"data\":[{\"id\":\"claude-x\"}]}");
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(body.clone())).await;
    let mut config = base_config(mock.base_url());
    config.models_passthrough = true;
    let (proxy_url, _h) = spawn_proxy(config).await;

    let resp = reqwest::get(format!("{proxy_url}/v1/models?limit=1000"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let received = resp.bytes().await.unwrap();
    assert_eq!(received.as_ref(), body.as_ref());

    mock.shutdown().await;
}

#[tokio::test]
async fn count_tokens_returns_404_without_reaching_upstream() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let resp = reqwest::Client::new()
        .post(format!("{proxy_url}/v1/messages/count_tokens"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert!(mock.requests().is_empty());

    mock.shutdown().await;
}

/// 設計 §3.1 #5: HEAD /api/hello はアップストリームへ往復せず即 200。
#[tokio::test]
async fn hello_probe_does_not_reach_upstream() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from_static(b"{}"))).await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let resp = reqwest::Client::new()
        .head(format!("{proxy_url}/api/hello"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(mock.requests().is_empty());

    mock.shutdown().await;
}

/// 設計 §13.3: upstream に到達できない場合は gedoku 独自の Anthropic 形式エラーを返す。
#[tokio::test]
async fn upstream_unreachable_returns_gedoku_error_envelope() {
    // 127.0.0.1:1 は原則接続できないポート (RFC 6335 予約域外だが慣習的に未使用)。
    let config = base_config("http://127.0.0.1:1".to_string());
    let (proxy_url, _h) = spawn_proxy(config).await;

    let resp = reqwest::Client::new()
        .post(format!("{proxy_url}/v1/messages"))
        .body("{}")
        .send()
        .await
        .unwrap();

    assert!(
        resp.status() == StatusCode::BAD_GATEWAY || resp.status() == StatusCode::GATEWAY_TIMEOUT
    );
    let body_bytes = resp.bytes().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body["type"], "error");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.starts_with("gedoku-proxy: "));
    assert!(!message.contains("capability_rejected:"));
}

/// P0-T9: 「直結」vs「gedoku経由」でレスポンスの生バイト列・ヘッダーが一致することを証明するハーネス。
#[tokio::test]
async fn p0_direct_vs_proxied_response_is_byte_identical() {
    let response_body = Bytes::from_static(
        br#"{"id":"msg_2","type":"message","content":[{"type":"text","text":"parity check"}]}"#,
    );
    let mock = MockUpstream::spawn(ResponseSpec::Fixed {
        status: StatusCode::OK,
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("anthropic-request-id".into(), "req_parity".into()),
        ],
        body: response_body.clone(),
    })
    .await;

    // 直結: upstream (mock) へ直接叩く。
    let direct_resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", mock.base_url()))
        .header("x-api-key", "direct-key")
        .body(r#"{"model":"claude-x"}"#)
        .send()
        .await
        .unwrap();
    let direct_status = direct_resp.status();
    let direct_request_id = direct_resp.headers().get("anthropic-request-id").cloned();
    let direct_body = direct_resp.bytes().await.unwrap();

    // gedoku 経由。
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;
    let proxied_resp = reqwest::Client::new()
        .post(format!("{proxy_url}/v1/messages"))
        .header("x-api-key", "direct-key")
        .body(r#"{"model":"claude-x"}"#)
        .send()
        .await
        .unwrap();
    let proxied_status = proxied_resp.status();
    let proxied_request_id = proxied_resp.headers().get("anthropic-request-id").cloned();
    let proxied_body = proxied_resp.bytes().await.unwrap();

    assert_eq!(direct_status, proxied_status);
    assert_eq!(direct_request_id, proxied_request_id);
    assert_eq!(direct_body.as_ref(), proxied_body.as_ref());

    let hash = |b: &[u8]| {
        let mut h = Sha256::new();
        h.update(b);
        hex::encode(h.finalize())
    };
    assert_eq!(hash(&direct_body), hash(&proxied_body));

    mock.shutdown().await;
}
