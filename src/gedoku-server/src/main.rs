use gedoku_audit::LogGuard;
use gedoku_core::Config;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // 設計 §10.3 M3/M4: コアダンプ抑制 (RLIMIT_CORE=0/PR_SET_DUMPABLE=0) を
    // プロセス起動直後に適用する。マスター鍵等の秘密をメモリに載せる前に行う。
    gedoku_mapstore::harden_process_best_effort();

    // 設計 §12.4: ログ出口ガードを1つだけ生成し、tracing の writer と
    // redact 経路の双方で共有する (Redactor が検出した原文をここへ登録する)。
    let log_guard = LogGuard::stdout();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .with_writer(log_guard.clone())
        .init();

    // 設計 §18 / 計画 P5-T1: `GEDOKU_CONFIG_FILE` が指すTOMLがあればそれを、
    // なければ環境変数から読み込む。不正な設定ファイルは起動を失敗させる。
    let config = Arc::new(Config::load());
    let listen_addr = config.listen_addr.clone();
    let upstream = config.upstream_base_url.clone();

    let app = gedoku_server::build_router_with_guard(config, log_guard);

    let listener = TcpListener::bind(&listen_addr)
        .await
        .unwrap_or_else(|e| panic!("gedoku-proxy: failed to bind {listen_addr}: {e}"));

    tracing::info!(listen_addr, upstream, "gedoku-proxy listening");

    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("gedoku-proxy: server error: {e}"));
}
