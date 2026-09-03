//! gedoku-server: axum ハンドラの配線 (計画 §3, P0 一式)。

pub mod proxy;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, head};
use axum::Router;
use gedoku_core::Config;
use http::StatusCode;
use proxy::{generic_proxy, AppState};
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

pub fn build_router_with_client(config: Arc<Config>, client: reqwest::Client) -> Router {
    let state = AppState { config, client };

    Router::new()
        .route("/health", get(health))
        .route("/api/hello", head(hello))
        .route(
            "/v1/messages/count_tokens",
            any(count_tokens_not_implemented),
        )
        .route("/v1/models", get(models))
        .fallback(any(generic_proxy))
        .with_state(state)
}
