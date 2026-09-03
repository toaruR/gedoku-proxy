//! プロセス硬化 (設計 §10.3 M3/M4)。ベストエフォート。
//!
//! `RLIMIT_CORE=0` / `PR_SET_DUMPABLE=0` / `mlock` は Linux 本番運用を前提とした
//! 要件であり、開発環境 (Windows 等) では意図的に no-op にする。失敗しても
//! プロセスを止めない (M系はここが唯一の防壁ではなく、値そのものを暗号化・
//! zeroize する多層防御の一枚として扱う)。

#[cfg(unix)]
pub fn harden_process_best_effort() {
    unsafe {
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        libc::setrlimit(libc::RLIMIT_CORE, &limit);
        #[cfg(target_os = "linux")]
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
}

#[cfg(not(unix))]
pub fn harden_process_best_effort() {}

/// マスター鍵領域を swap へ出さないための best-effort `mlock` (§10.3 M3)。
#[cfg(unix)]
pub fn mlock_best_effort(bytes: &[u8]) {
    unsafe {
        libc::mlock(bytes.as_ptr() as *const libc::c_void, bytes.len());
    }
}

#[cfg(not(unix))]
pub fn mlock_best_effort(_bytes: &[u8]) {}
