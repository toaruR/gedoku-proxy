//! P3 (ストリーム復元) の DoD を検証する統合テスト群。
//! 計画 §5.3 の P3-T3/T5 に対応する。
//!
//! mock upstream が「受信した (redact 済み) リクエストボディをそのままエコーする」
//! モードで、任意のバイト境界に分割してストリーミング応答する。これにより、
//! アップストリームがプレースホルダー文字列をレスポンス中にエコーバックする
//! (モデルが会話履歴中のプレースホルダーをそのまま繰り返す) 実運用シナリオと、
//! それが SSE チャンク境界をまたいで分割されるケースの両方を1本のリクエストで
//! 再現できる。

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

/// P3-T5 DoD 相当: リクエストで検出/置換された秘密が、アップストリームの
/// エコー応答経由でクライアントに戻ってくる際、プレースホルダーではなく原文へ
/// 復元されていること。
#[tokio::test]
async fn placeholder_echoed_in_response_is_restored_to_original_secret() {
    let mock = MockUpstream::spawn(ResponseSpec::EchoRequestBody {
        status: StatusCode::OK,
        headers: vec![("content-type".into(), "text/event-stream".into())],
        // チャンク境界網羅 (T-5) 相当: 5バイトごとに分割し、確実にプレースホルダー
        // の途中で複数回チャンクが切れるようにする。
        split_at_bytes: (5..4096).step_by(5).collect(),
        delay_between: Duration::ZERO,
    })
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
        "sanity check: upstream must still never see the original secret"
    );
    assert!(upstream_body.contains("\u{27E6}GDK:SECRET:"));

    let client_body = resp.text().await.unwrap();
    assert!(
        client_body.contains(secret),
        "client must receive the original secret restored from the placeholder"
    );
    assert!(
        !client_body.contains("\u{27E6}GDK:"),
        "client must never see the raw placeholder sentinel once resolvable"
    );
    // 復元後もエコーされたボディの非プレースホルダー部分はバイト同一 (無検閲原則)。
    assert!(client_body.contains("\"model\":\"claude-x\""));

    mock.shutdown().await;
}

/// P3-T3 DoD 相当: プレースホルダーの全バイト位置での分割 (span 内の全通り) で
/// 復元結果が壊れないことを、実際の HTTP 経由 (SSE チャンク境界相当) で確認する。
/// 分割点はまず (ネットワークを介さず) `Redactor` を直接呼んでプレースホルダーの
/// 実際のバイト範囲を求め、その範囲内の全オフセットだけを走査する
/// (JSON 中の位置は決定的、ID 文字列自体は各テスト実行の proxy ごとに異なってよい)。
#[tokio::test]
async fn placeholder_split_at_every_byte_offset_still_restores() {
    let secret = "hunter2-secret-value";
    let request_body = serde_json::json!({
        "model": "claude-x",
        "max_tokens": 16,
        "messages": [{"role": "user", "content": format!("token={secret}")}]
    });
    let request_bytes = serde_json::to_vec(&request_body).unwrap();

    let realm = gedoku_core::RealmId::new("anonymous");
    let probe = gedoku_server::redact::Redactor::new();
    let redacted = String::from_utf8(
        probe
            .redact_body(&request_bytes, &realm, "/v1/messages")
            .unwrap(),
    )
    .unwrap();
    let ph_start = redacted
        .find("\u{27E6}GDK:SECRET:")
        .expect("secret must be redacted for this fixture");
    let ph_end = ph_start
        + redacted[ph_start..]
            .find('\u{27E7}')
            .expect("placeholder must be closed")
        + '\u{27E7}'.len_utf8();

    for split in ph_start..=ph_end {
        let mock = MockUpstream::spawn(ResponseSpec::EchoRequestBody {
            status: StatusCode::OK,
            headers: vec![],
            split_at_bytes: vec![split],
            delay_between: Duration::ZERO,
        })
        .await;
        let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{proxy_url}/v1/messages"))
            .header("content-type", "application/json")
            .body(request_bytes.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let client_body = resp.text().await.unwrap();
        assert!(
            client_body.contains(secret),
            "failed to restore secret when split at byte {split}: {client_body}"
        );
        assert!(
            !client_body.contains("\u{27E6}GDK:"),
            "raw placeholder leaked to client when split at byte {split}"
        );

        mock.shutdown().await;
    }
}

/// P3 のフェイルオープン方針: マッピングストアに存在しない (別 realm や
/// 手作りの) プレースホルダー文字列は、解決できないままでも壊さず素通しする
/// (秘密を推測・捏造しない)。
#[tokio::test]
async fn unknown_placeholder_in_response_passes_through_unresolved() {
    let mock = MockUpstream::spawn(ResponseSpec::json_ok(Bytes::from(
        "\u{27E6}GDK:SECRET:0000000000\u{27E7}".as_bytes().to_vec(),
    )))
    .await;
    let (proxy_url, _h) = spawn_proxy(base_config(mock.base_url())).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{proxy_url}/v1/messages"))
        .header("content-type", "application/json")
        .body(
            serde_json::to_vec(&serde_json::json!({
                "model": "claude-x",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .unwrap(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let client_body = resp.text().await.unwrap();
    assert_eq!(client_body, "\u{27E6}GDK:SECRET:0000000000\u{27E7}");

    mock.shutdown().await;
}
