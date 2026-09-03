//! 透過プロキシ本体 (設計 §4, §5, §13 / 計画 P0)。
//!
//! P0 の範囲では検出・置換を一切行わない。リクエスト／レスポンスのボディは
//! ストリームのまま素通しし、ヘッダーはオープンリスト方針 (§4.1) に従う。

use axum::body::Body;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use gedoku_core::{AnthropicError, AuthMode, Config};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use http_body_util::BodyDataStream;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub client: reqwest::Client,
}

/// hop-by-hop ヘッダー (RFC 9110)。設計 §4.3 DROP_HOPBYHOP。
const DROP_HOPBYHOP: &[&str] = &[
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "proxy-authenticate",
    "proxy-authorization",
];

/// リクエスト転送時にのみ落とす (再構築するため)。設計 §4.3 DROP_EXACT。
const DROP_EXACT_REQUEST: &[&str] = &["host", "content-length", "accept-encoding"];

fn is_hop_by_hop(name: &str, connection_header_tokens: &[String]) -> bool {
    let lname = name.to_ascii_lowercase();
    DROP_HOPBYHOP.contains(&lname.as_str()) || connection_header_tokens.iter().any(|t| t == &lname)
}

fn connection_header_tokens(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// 設計 §4.3 build_upstream_headers の Rust 実装。
fn build_upstream_headers(config: &Config, in_headers: &HeaderMap) -> HeaderMap {
    let conn_tokens = connection_header_tokens(in_headers);
    let mut out = HeaderMap::new();

    for (name, value) in in_headers.iter() {
        let lname = name.as_str().to_ascii_lowercase();
        if is_hop_by_hop(&lname, &conn_tokens) {
            continue;
        }
        if DROP_EXACT_REQUEST.contains(&lname.as_str()) {
            continue;
        }
        if lname == "authorization" || lname == "x-api-key" {
            continue; // apply_credentials で別途組み立てる
        }
        out.append(name.clone(), value.clone());
    }

    out.insert(
        "accept-encoding",
        HeaderValue::from_str(&config.upstream_accept_encoding)
            .unwrap_or(HeaderValue::from_static("identity")),
    );

    apply_credentials(&mut out, in_headers, config);
    out
}

/// 設計 §5.2 apply_credentials の Rust 実装。
fn apply_credentials(out: &mut HeaderMap, in_headers: &HeaderMap, config: &Config) {
    let copy_if_present = |out: &mut HeaderMap, name: &'static str| {
        if let Some(v) = in_headers.get(name) {
            out.insert(HeaderName::from_static(name), v.clone());
        }
    };

    match config.auth_mode {
        AuthMode::Passthrough => {
            copy_if_present(out, "authorization");
            copy_if_present(out, "x-api-key");
        }
        AuthMode::Replace => {
            if let Some(token) = &config.upstream_auth_token {
                if let Ok(v) = HeaderValue::from_str(&format!("Bearer {token}")) {
                    out.insert(HeaderName::from_static("authorization"), v);
                }
            }
            if let Some(key) = &config.upstream_api_key {
                if let Ok(v) = HeaderValue::from_str(key) {
                    out.insert(HeaderName::from_static("x-api-key"), v);
                }
            }
        }
        AuthMode::Augment => {
            copy_if_present(out, "authorization");
            copy_if_present(out, "x-api-key");
            for (name, value) in &config.extra_upstream_headers {
                if let (Ok(n), Ok(v)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    out.insert(n, v);
                }
            }
        }
    }
}

/// レスポンスヘッダーの開放リスト転送 (設計 GAP-6反映)。hop-by-hop のみ落とす。
fn build_downstream_headers(upstream_headers: &http::HeaderMap) -> HeaderMap {
    let conn_tokens: Vec<String> = upstream_headers
        .get_all(http::header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    let mut out = HeaderMap::new();
    for (name, value) in upstream_headers.iter() {
        let lname = name.as_str().to_ascii_lowercase();
        if is_hop_by_hop(&lname, &conn_tokens) {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

fn build_upstream_url(config: &Config, uri: &http::Uri) -> String {
    let base = config.upstream_base_url.trim_end_matches('/');
    match uri.path_and_query() {
        Some(pq) => format!("{base}{}", pq.as_str()),
        None => format!("{base}{}", uri.path()),
    }
}

/// 汎用透過プロキシハンドラ。P0 の全パスがここを通る (§3.1 #9 既定 passthrough)。
pub async fn generic_proxy(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    proxy_request(&state, req).await
}

pub async fn proxy_request(state: &AppState, req: axum::extract::Request) -> Response {
    let (parts, body) = req.into_parts();
    let upstream_url = build_upstream_url(&state.config, &parts.uri);
    let upstream_headers = build_upstream_headers(&state.config, &parts.headers);

    let body_stream = BodyDataStream::new(body);
    let upstream_body = reqwest::Body::wrap_stream(body_stream);

    let request = state
        .client
        .request(parts.method.clone(), &upstream_url)
        .headers(upstream_headers)
        .body(upstream_body);

    match request.send().await {
        Ok(upstream_resp) => stream_response(upstream_resp).await,
        Err(err) => connect_error_response(&err),
    }
}

async fn stream_response(upstream_resp: reqwest::Response) -> Response {
    let status = upstream_resp.status();
    let headers = build_downstream_headers(upstream_resp.headers());
    let body = Body::from_stream(upstream_resp.bytes_stream());

    let mut builder = Response::builder().status(status);
    for (name, value) in headers.iter() {
        builder = builder.header(name, value);
    }
    builder.body(body).unwrap_or_else(|_| {
        (
            StatusCode::BAD_GATEWAY,
            "gedoku-proxy: failed to build response",
        )
            .into_response()
    })
}

/// 設計 §13.3: アップストリームに到達しなかった場合のみプロキシがエラーを生成する。
fn connect_error_response(err: &reqwest::Error) -> Response {
    let status = if err.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    };
    let body = AnthropicError::api_error(format!("upstream connect failed: {err}"));
    let json = serde_json::to_vec(&body).unwrap_or_default();
    (
        status,
        [("content-type", "application/json")],
        Bytes::from(json),
    )
        .into_response()
}

pub fn body_bytes_error_response(status: StatusCode, error: AnthropicError) -> Response {
    let json = serde_json::to_vec(&error).unwrap_or_default();
    (
        status,
        [("content-type", "application/json")],
        Bytes::from(json),
    )
        .into_response()
}
