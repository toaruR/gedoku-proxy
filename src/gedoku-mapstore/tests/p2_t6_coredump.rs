//! 計画 P2-T6: プロセスのコアダンプを強制生成し、`strings` でダンプ内に
//! 平文原文が0件であることを確認するテスト。
//!
//! Linux 専用。CI 側で以下を満たすこと:
//! - `/proc/sys/kernel/core_pattern` が固定パス形式(`%p` を含む)であること
//! - `strings` コマンドが利用可能であること
//! 満たさない環境では known-gaps.md の通り実行不可のため `#[ignore]` する。

#![cfg(all(unix, target_os = "linux"))]

use gedoku_core::RealmId;
use gedoku_mapstore::{MappingStore, MappingStoreConfig, MemoryMappingStore, SecretString};

#[test]
#[ignore = "Linux専用・core_pattern/rootが必要な環境限定 (docs/known-gaps.md P2-T6参照)"]
fn forced_coredump_contains_no_plaintext_secret() {
    let marker = format!(
        "GEDOKU-P2T6-MARKER-{}-DO-NOT-LEAK",
        std::process::id()
    );

    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");

    if pid == 0 {
        // 子プロセス: コアダンプ制限を解除し、秘密値を扱ってから強制クラッシュする。
        unsafe {
            let unlimited = libc::rlimit {
                rlim_cur: libc::RLIM_INFINITY,
                rlim_max: libc::RLIM_INFINITY,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &unlimited);
        }

        let store = MemoryMappingStore::with_master_key([7u8; 32], MappingStoreConfig::default());
        let realm = RealmId::new("p2-t6-coredump-realm");
        let placeholder =
            store.get_or_create(&realm, "SECRET", SecretString::new(marker.clone()));
        let resolved = store
            .resolve(&realm, &placeholder)
            .expect("must resolve just-stored secret");
        assert_eq!(resolved.expose_secret(), marker);

        // 生存中のスコープをすべて畳んでゼロ化を発生させてからダンプする。
        drop(resolved);
        drop(store);
        drop(placeholder);
        drop(marker);

        unsafe {
            libc::raise(libc::SIGABRT);
            // raise が返ってきたら異常なので明示的に失敗コードで終了する
            // (親プロセス側のアサーションに委ねず、二重に検知できるようにする)。
            libc::_exit(111);
        }
    }

    // 親プロセス: 子の終了を待ち、コアダンプを回収して strings で走査する。
    let mut status: i32 = 0;
    let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(waited, pid, "waitpid failed");
    assert!(
        libc_wifsignaled(status) && libc_wtermsig(status) == libc::SIGABRT,
        "child did not die from SIGABRT as expected (status={status})"
    );

    let core_path = find_core_file(pid).unwrap_or_else(|| {
        panic!(
            "coredump file for pid {pid} not found under /tmp; \
             check /proc/sys/kernel/core_pattern is a plain path containing %p"
        )
    });

    let output = std::process::Command::new("strings")
        .arg(&core_path)
        .output()
        .expect("failed to run `strings` on coredump");
    assert!(output.status.success(), "strings command failed");

    let dump_text = String::from_utf8_lossy(&output.stdout);
    let hits = dump_text.matches(&marker).count();

    std::fs::remove_file(&core_path).ok();

    assert_eq!(
        hits, 0,
        "plaintext secret marker found {hits} time(s) in forced coredump"
    );
}

fn find_core_file(pid: i32) -> Option<std::path::PathBuf> {
    let needle = pid.to_string();
    for entry in std::fs::read_dir("/tmp").ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("core.") && name.ends_with(&needle) {
            return Some(entry.path());
        }
    }
    None
}

fn libc_wifsignaled(status: i32) -> bool {
    (status & 0x7f) != 0 && (status & 0x7f) != 0x7f
}

fn libc_wtermsig(status: i32) -> i32 {
    status & 0x7f
}
