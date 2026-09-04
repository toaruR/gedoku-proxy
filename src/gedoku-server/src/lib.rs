//! gedoku-server: axum ハンドラの配線 (計画 §3, P0 一式)。

pub mod proxy;
pub mod redact;
pub mod restore;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, head, post};
use axum::Router;
use gedoku_audit::LogGuard;
use gedoku_core::Config;
use http::StatusCode;
use proxy::{generic_proxy, AppState};
use redact::Redactor;
use std::sync::Arc;

/// 設計 §3.1 #1: `/v1/health` 相当のヘルスチェック (P0-T1)。
async fn health() -> &'static str {
    "ok"
}

/// 設計 §3.1 #5: HEAD /api/hello はアップストリームへ往復せず即 200 を返す。
async fn hello() -> Response {
    StatusCode::OK.into_response()
}

/// 設計 §3.1 #3, §3.3: count_tokens は v1 では実装せず明示的に 404 を返す。
/// Claude Code は自動で messages 経由のフォールバックへ落ちるため機能破壊は起きない。
async fn count_tokens_not_implemented() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// 設計 §3.1 #4, §3.4: /v1/models は既定 OFF。有効化時のみアップストリームへ透過する。
async fn models(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    if state.config.models_passthrough {
        proxy::proxy_request(&state, req).await
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

pub fn build_client(config: &Config) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(config.upstream_connect_timeout)
        // レスポンスは SSE を含み長時間張りっぱなしになるため read timeout は設けない (既定で無制限)。
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build reqwest client")
}

pub fn build_router(config: Arc<Config>) -> Router {
    let client = build_client(&config);
    build_router_with_client(config, client)
}

/// `main.rs` から呼ぶ配線。`log_guard` は `tracing_subscriber` の writer と共有
/// する必要があるため、呼び出し側で1つ生成したインスタンスを受け取る
/// (設計 §12.4、計画 P4-T2)。
pub fn build_router_with_guard(config: Arc<Config>, log_guard: LogGuard) -> Router {
    let client = build_client(&config);
    let redactor = Arc::new(Redactor::with_log_guard_and_debug_dump(
        log_guard,
        config.debug_dump_replacements,
    ));
    let state = AppState {
        config,
        client,
        redactor,
    };
    build_router_from_state(state)
}

/// 設計 §7.9: `/v1/messages` はボディ全体を読み切って検出/置換を行う専用経路。
async fn messages(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    proxy::redacted_messages_proxy(&state, req).await
}

pub fn build_router_with_client(config: Arc<Config>, client: reqwest::Client) -> Router {
    let redactor = Arc::new(Redactor::with_log_guard_and_debug_dump(
        LogGuard::stdout(),
        config.debug_dump_replacements,
    ));
    let state = AppState {
        config,
        client,
        redactor,
    };
    build_router_from_state(state)
}

fn build_router_from_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/hello", head(hello))
        .route(
            "/v1/messages/count_tokens",
            any(count_tokens_not_implemented),
        )
        .route("/v1/messages", post(messages))
        .route("/v1/models", get(models))
        .fallback(any(generic_proxy))
        .with_state(state)
}
