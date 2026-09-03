//! アップストリーム応答ストリーム内のプレースホルダーを原文へ復元する
//! (設計 §12, 計画 P3)。
//!
//! `/v1/messages` はリクエストで検出/置換 (P2) を行うため、モデルがプレースホルダー
//! 文字列をそのままレスポンスへエコーバックすることがある。このモジュールは SSE
//! チャンク境界をまたいで分割されたプレースホルダーも `gedoku-stream` の tail-hold
//! バッファで復元してからクライアントへ転送する。

use crate::proxy::build_downstream_headers;
use crate::redact::Redactor;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::StreamExt;
use gedoku_core::RealmId;
use gedoku_stream::TailHoldScanner;
use http::StatusCode;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// §12.2 restoration イベント用の単一バケット名。tail-hold スキャナはカテゴリを
/// 呼び出し側へ渡さない (ID のみ) ため、カテゴリ別ではなく総数のみを集計する。
const RESTORED_BUCKET: &str = "RESTORED";

/// tail-hold 保留中のバイト列を無条件で吐き出すまでの最大待ち時間
/// (設計 §12 の 50ms 上限。バイト上限は `gedoku_stream::MAX_HOLD_BYTES` 側)。
const STALE_HOLD_TIMEOUT: Duration = Duration::from_millis(50);

pub async fn restore_response(
    upstream_resp: reqwest::Response,
    redactor: Arc<Redactor>,
    realm: RealmId,
) -> Response {
    let status = upstream_resp.status();
    let headers = build_downstream_headers(upstream_resp.headers());
    let mut upstream = upstream_resp.bytes_stream();

    let body_stream = async_stream::stream! {
        let mut scanner = TailHoldScanner::new();
        let restored_count = AtomicU64::new(0);
        let miss_count = AtomicU64::new(0);
        loop {
            let next = if scanner.has_pending() {
                match tokio::time::timeout(STALE_HOLD_TIMEOUT, upstream.next()).await {
                    Ok(item) => item,
                    Err(_elapsed) => {
                        let out = scanner.flush();
                        if !out.is_empty() {
                            yield Ok::<_, std::io::Error>(Bytes::from(out));
                        }
                        continue;
                    }
                }
            } else {
                upstream.next().await
            };

            match next {
                Some(Ok(chunk)) => {
                    let resolver = |id: &str| {
                        let resolved = redactor.resolve_placeholder(&realm, id);
                        match &resolved {
                            Some(_) => {
                                restored_count.fetch_add(1, Ordering::Relaxed);
                            }
                            None => {
                                miss_count.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        resolved
                    };
                    let out = scanner.feed(&chunk, &resolver);
                    if !out.is_empty() {
                        yield Ok(Bytes::from(out));
                    }
                }
                Some(Err(err)) => {
                    yield Err(std::io::Error::other(err.to_string()));
                    break;
                }
                None => {
                    let out = scanner.flush();
                    if !out.is_empty() {
                        yield Ok(Bytes::from(out));
                    }
                    break;
                }
            }
        }

        let mut restored: BTreeMap<&'static str, u64> = BTreeMap::new();
        let restored_total = restored_count.load(Ordering::Relaxed);
        if restored_total > 0 {
            restored.insert(RESTORED_BUCKET, restored_total);
        }
        redactor.emit_restoration_event(&realm, restored, miss_count.load(Ordering::Relaxed));
    };

    let body = Body::from_stream(body_stream);
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
