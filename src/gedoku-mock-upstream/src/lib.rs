//! gedoku-mock-upstream: 偽 Anthropic サーバ (計画 §8)。
//!
//! P0 検証に必要な最小機能のみを提供する:
//! - 受信した method/uri/headers/body の記録 (ヘッダー転送・認証転送のテスト用)
//! - 固定レスポンス (JSON / エラー) の返却
//! - SSE 風の複数チャンクを、送出タイミング・バイト境界を指定して返却 (ストリーミング透過のテスト用)

use axum::body::Body;
use axum::extract::{Request, State};
use axum::response::Response;
use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: Method,
    pub uri: Uri,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl RecordedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

#[derive(Clone)]
pub enum ResponseSpec {
    /// 固定ステータス/ヘッダー/ボディを一度に返す (非ストリーム応答・エラー応答用)
    Fixed {
        status: StatusCode,
        headers: Vec<(String, String)>,
        body: Bytes,
    },
    /// 複数チャンクを順に flush しながら返す (SSE 等のストリーミング透過検証用)
    Chunked {
        status: StatusCode,
        headers: Vec<(String, String)>,
        chunks: Vec<Bytes>,
        delay_between: Duration,
    },
    /// 受信したリクエストボディをそのままレスポンスとしてエコーし、指定した
    /// バイト境界 (昇順) で複数チャンクに分割して送出する (計画 §8.2
    /// `split_at_bytes` 相当、P3: ストリーム復元のチャンク境界網羅テスト用)。
    EchoRequestBody {
        status: StatusCode,
        headers: Vec<(String, String)>,
        split_at_bytes: Vec<usize>,
        delay_between: Duration,
    },
}

impl ResponseSpec {
    pub fn json_ok(body: impl Into<Bytes>) -> Self {
        ResponseSpec::Fixed {
            status: StatusCode::OK,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.into(),
        }
    }

    pub fn error(status: StatusCode, body: impl Into<Bytes>) -> Self {
        ResponseSpec::Fixed {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.into(),
        }
    }
}

struct MockState {
    response: ResponseSpec,
    requests: Mutex<Vec<RecordedRequest>>,
}

pub struct MockUpstream {
    pub addr: SocketAddr,
    state: Arc<MockState>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: JoinHandle<()>,
}

impl MockUpstream {
    pub async fn spawn(response: ResponseSpec) -> Self {
        let state = Arc::new(MockState {
            response,
            requests: Mutex::new(Vec::new()),
        });

        let app = axum::Router::new()
            .fallback(handler)
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let addr = listener.local_addr().expect("local_addr");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let join = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("mock upstream serve");
        });

        MockUpstream {
            addr,
            state,
            shutdown_tx: Some(shutdown_tx),
            join,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state.requests.lock().expect("lock").clone()
    }

    pub fn last_request(&self) -> Option<RecordedRequest> {
        self.state.requests.lock().expect("lock").last().cloned()
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = (&mut self.join).await;
    }
}

async fn handler(State(state): State<Arc<MockState>>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let body_bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();

    state.requests.lock().expect("lock").push(RecordedRequest {
        method: parts.method.clone(),
        uri: parts.uri.clone(),
        headers: parts.headers.clone(),
        body: body_bytes.clone(),
    });

    match state.response.clone() {
        ResponseSpec::Fixed {
            status,
            headers,
            body,
        } => {
            let mut builder = Response::builder().status(status);
            for (name, value) in headers {
                builder = builder.header(
                    HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                    HeaderValue::from_str(&value).expect("header value"),
                );
            }
            builder.body(Body::from(body)).expect("build response")
        }
        ResponseSpec::Chunked {
            status,
            headers,
            chunks,
            delay_between,
        } => {
            let mut builder = Response::builder().status(status);
            for (name, value) in headers {
                builder = builder.header(
                    HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                    HeaderValue::from_str(&value).expect("header value"),
                );
            }
            let stream = async_stream::stream! {
                for chunk in chunks {
                    if !delay_between.is_zero() {
                        tokio::time::sleep(delay_between).await;
                    }
                    yield Ok::<_, std::io::Error>(chunk);
                }
            };
            builder
                .body(Body::from_stream(stream))
                .expect("build response")
        }
        ResponseSpec::EchoRequestBody {
            status,
            headers,
            split_at_bytes,
            delay_between,
        } => {
            let mut builder = Response::builder().status(status);
            for (name, value) in headers {
                builder = builder.header(
                    HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                    HeaderValue::from_str(&value).expect("header value"),
                );
            }
            let chunks = split_bytes(&body_bytes, &split_at_bytes);
            let stream = async_stream::stream! {
                for chunk in chunks {
                    if !delay_between.is_zero() {
                        tokio::time::sleep(delay_between).await;
                    }
                    yield Ok::<_, std::io::Error>(chunk);
                }
            };
            builder
                .body(Body::from_stream(stream))
                .expect("build response")
        }
    }
}

/// `points` (昇順のバイト境界) で `body` を分割する。境界は自動的に
/// `[0, body.len()]` の範囲へクランプ・重複排除される。
fn split_bytes(body: &Bytes, points: &[usize]) -> Vec<Bytes> {
    let mut bounds: Vec<usize> = std::iter::once(0)
        .chain(points.iter().copied().map(|p| p.min(body.len())))
        .chain(std::iter::once(body.len()))
        .collect();
    bounds.dedup();
    bounds
        .windows(2)
        .map(|w| body.slice(w[0]..w[1]))
        .filter(|c| !c.is_empty())
        .collect()
}
