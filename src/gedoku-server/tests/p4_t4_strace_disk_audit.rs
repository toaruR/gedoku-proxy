//! P4-T4: ディスク非書き込み監査 (計画 §5.4, リスク表 RISK-4)。
//!
//! `strace -e trace=openat,write` でプロセス全体のシステムコールを記録し、
//! 合成秘密コーパスの原文が openat/write システムコールのトレースに一切
//! 現れないことを確認する。Linux + `strace` が使える環境限定 (縮小版は
//! `p4_audit_log.rs` のプロセス内ログ捕捉、本テストはOSシステムコール境界での
//! 独立検証)。`strace` が無い環境ではスキップする (docs/known-gaps.md 参照)。

#![cfg(target_os = "linux")]

use gedoku_core::RealmId;
use gedoku_server::redact::Redactor;
use std::io::Write as _;
use std::process::Command;

const CHILD_ENV: &str = "GEDOKU_P4T4_STRACE_CHILD";
const CORPUS: &[&str] = &[
    "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcd",
    "jane.doe@example-corp.internal",
    "AKIAABCDEFGHIJKLMNOP",
    "4111 1111 1111 1111",
    "hunter2-super-secret-token-value",
];

#[test]
fn disk_io_syscalls_never_carry_plaintext_secrets() {
    // 自分自身を `strace` 配下で再実行する。子プロセス側 (この env var が
    // 立っている側) は実際のワークロードを実行するだけで、トレースの回収・
    // 検証は親側 (env var なし) が行う。
    if std::env::var(CHILD_ENV).is_ok() {
        run_secret_workload();
        return;
    }

    if which("strace").is_none() {
        eprintln!("strace not found on PATH; skipping P4-T4 (see docs/known-gaps.md)");
        return;
    }

    let trace_path = std::env::temp_dir().join(format!("p4_t4_strace_{}.log", std::process::id()));
    let exe = std::env::current_exe().expect("current_exe");

    let status = Command::new("strace")
        .args([
            "-f",
            "-s",
            "65536",
            "-e",
            "trace=openat,open,write,pwrite64,writev",
            "-o",
        ])
        .arg(&trace_path)
        .arg(&exe)
        .args([
            "disk_io_syscalls_never_carry_plaintext_secrets",
            "--exact",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .status()
        .expect("failed to launch strace (is it installed?)");
    assert!(
        status.success(),
        "traced child process exited with failure: {status:?}"
    );

    let trace = std::fs::read_to_string(&trace_path)
        .unwrap_or_else(|e| panic!("failed to read strace output at {trace_path:?}: {e}"));
    std::fs::remove_file(&trace_path).ok();

    for secret in CORPUS {
        assert!(
            !trace.contains(secret),
            "disk I/O syscall trace leaked plaintext secret: {secret}"
        );
    }
    assert!(
        trace.contains("openat(") || trace.contains("write("),
        "expected openat/write syscalls to have been traced at all (harness sanity check)"
    );
}

fn run_secret_workload() {
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stdout)
        .finish();
    let redactor = Redactor::new();
    let realm = RealmId::new("p4-t4-strace-realm");

    tracing::subscriber::with_default(subscriber, || {
        for secret in CORPUS {
            let body = serde_json::json!({
                "model": "claude-x",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": format!("value: {secret}")}]
            });
            let bytes = serde_json::to_vec(&body).unwrap();
            let _ = redactor.redact_body(&bytes, &realm, "/v1/messages");
        }
    });
    let _ = std::io::stdout().flush();
}

fn which(cmd: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(cmd);
            candidate.is_file().then_some(candidate)
        })
    })
}
