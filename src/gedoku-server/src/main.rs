use gedoku_core::Config;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let config = Arc::new(Config::from_env());
    let listen_addr = config.listen_addr.clone();
    let upstream = config.upstream_base_url.clone();

    let app = gedoku_server::build_router(config);

    let listener = TcpListener::bind(&listen_addr)
        .await
        .unwrap_or_else(|e| panic!("gedoku-proxy: failed to bind {listen_addr}: {e}"));

    tracing::info!(listen_addr, upstream, "gedoku-proxy listening");

    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("gedoku-proxy: server error: {e}"));
}
