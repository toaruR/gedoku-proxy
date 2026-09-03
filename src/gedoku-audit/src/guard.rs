//! ログ出口フィルタ (設計 §12.4)。
//!
//! 「設計の穴を運用で塞ぐ最後の防壁」— アプリケーションコードが誤って原文を
//! ログに渡してしまった場合でも、実際に書き出される直前のバイト列を既知の
//! 原文リストと照合し、一致すればレコード全体を `<redacted-by-log-guard>` へ
//! 置換する。`tracing_subscriber::fmt` の `MakeWriter` として配線する。

use aho_corasick::AhoCorasick;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

const REDACTED_MARKER: &[u8] = b"<redacted-by-log-guard>\n";

struct Patterns {
    secrets: Vec<String>,
    automaton: AhoCorasick,
}

impl Patterns {
    fn empty() -> Self {
        Patterns {
            secrets: Vec::new(),
            automaton: AhoCorasick::new(std::iter::empty::<&str>())
                .expect("empty automaton always builds"),
        }
    }
}

/// クローン可能なログ出口ガード。内部状態は `Arc` 共有のため、
/// `tracing_subscriber` の `MakeWriter::make_writer` が呼ぶたびクローンしても
/// 同一の照合対象リスト・トリップカウンタを参照する。
#[derive(Clone)]
pub struct LogGuard {
    patterns: Arc<RwLock<Patterns>>,
    sink: Arc<Mutex<dyn io::Write + Send>>,
    trips: Arc<AtomicU64>,
}

impl LogGuard {
    /// 任意の書き込み先をラップする。
    pub fn wrapping<W: io::Write + Send + 'static>(sink: W) -> Self {
        LogGuard {
            patterns: Arc::new(RwLock::new(Patterns::empty())),
            sink: Arc::new(Mutex::new(sink)),
            trips: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 標準出力へ書き出す既定のガード。
    pub fn stdout() -> Self {
        Self::wrapping(io::stdout())
    }

    /// 原文をガードの照合対象として登録する。既知の値は再登録しない。
    /// この呼び出し自体はログに何も出力しない。
    pub fn register_secret(&self, plaintext: &str) {
        if plaintext.is_empty() {
            return;
        }
        let mut guard = self.patterns.write().unwrap_or_else(|e| e.into_inner());
        if guard.secrets.iter().any(|s| s == plaintext) {
            return;
        }
        guard.secrets.push(plaintext.to_string());
        guard.automaton = AhoCorasick::new(guard.secrets.iter())
            .expect("registered secrets always build a valid automaton");
    }

    /// これまでにガードが本文を差し替えた回数。0 以外は重大アラート
    /// (設計 §12.5 `gedoku_log_guard_trips_total`)。
    pub fn trips(&self) -> u64 {
        self.trips.load(Ordering::Relaxed)
    }
}

impl io::Write for LogGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let requested_len = buf.len();
        let hit = self
            .patterns
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .automaton
            .is_match(buf);

        let mut sink = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        if hit {
            self.trips.fetch_add(1, Ordering::Relaxed);
            sink.write_all(REDACTED_MARKER)?;
        } else {
            sink.write_all(buf)?;
        }
        Ok(requested_len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sink.lock().unwrap_or_else(|e| e.into_inner()).flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogGuard {
    type Writer = LogGuard;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn shared_buffer() -> (Arc<Mutex<Vec<u8>>>, LogGuard) {
        struct SharedWriter(Arc<Mutex<Vec<u8>>>);
        impl io::Write for SharedWriter {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let buf = Arc::new(Mutex::new(Vec::new()));
        let guard = LogGuard::wrapping(SharedWriter(buf.clone()));
        (buf, guard)
    }

    #[test]
    fn passes_through_lines_without_registered_secrets() {
        let (buf, mut guard) = shared_buffer();
        io::Write::write_all(&mut guard, b"hello world\n").unwrap();
        assert_eq!(buf.lock().unwrap().as_slice(), b"hello world\n");
        assert_eq!(guard.trips(), 0);
    }

    #[test]
    fn redacts_and_counts_trip_when_registered_secret_appears() {
        let (buf, mut guard) = shared_buffer();
        guard.register_secret("sk-ant-verysecretvalue");
        io::Write::write_all(
            &mut guard,
            b"{\"msg\":\"leaked sk-ant-verysecretvalue here\"}\n",
        )
        .unwrap();
        let out = buf.lock().unwrap();
        assert!(!out.windows(23).any(|w| w == b"sk-ant-verysecretvalue"));
        assert_eq!(out.as_slice(), b"<redacted-by-log-guard>\n");
        assert_eq!(guard.trips(), 1);
    }

    #[test]
    fn unrelated_lines_after_a_trip_still_pass_through() {
        let (buf, mut guard) = shared_buffer();
        guard.register_secret("hunter2-secret");
        io::Write::write_all(&mut guard, b"contains hunter2-secret\n").unwrap();
        io::Write::write_all(&mut guard, b"clean line\n").unwrap();
        let out = buf.lock().unwrap();
        assert_eq!(
            out.as_slice(),
            [REDACTED_MARKER, b"clean line\n".as_slice()].concat()
        );
        assert_eq!(guard.trips(), 1);
    }
}
