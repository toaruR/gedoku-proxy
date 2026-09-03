//! P4 (監査ログ) の DoD を検証する統合テスト群 (計画 §5.4)。
//!
//! T-13 (全ログ出力 grep によるゼロ漏洩監査) の縮小版: `strace`/実 CI runner
//! ではなく、プロセス内でログ出力先を捕捉し合成秘密コーパスで grep する。
//! `gedoku-audit::LogGuard` の単体テスト (最後の防壁) と合わせて二層で検証する。

use gedoku_core::RealmId;
use gedoku_server::redact::Redactor;
use std::io;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone)]
struct CapturingWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CapturingWriter {
    type Writer = CapturingWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// P4-T5 (縮小版): 合成秘密コーパスを含むリクエストを `redact_body` (= 監査イベント
/// 発行経路) に通し、その間に発生した全ログ出力を捕捉して原文でgrepし、
/// ヒット数が0であることを確認する (design §12.3 の禁止リスト遵守の実証)。
#[test]
fn redaction_audit_log_never_contains_original_secret_values() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = CapturingWriter(buf.clone());
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(writer)
        .with_max_level(tracing::Level::TRACE)
        .finish();

    let redactor = Redactor::new();
    let realm = RealmId::new("audit-test-realm");

    let corpus = [
        "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcd",
        "jane.doe@example-corp.internal",
        "AKIAABCDEFGHIJKLMNOP",
        "4111 1111 1111 1111",
        "hunter2-super-secret-token-value",
    ];

    tracing::subscriber::with_default(subscriber, || {
        for secret in corpus {
            let body = serde_json::json!({
                "model": "claude-x",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": format!("value: {secret}")}]
            });
            let bytes = serde_json::to_vec(&body).unwrap();
            let _ = redactor.redact_body(&bytes, &realm, "/v1/messages");
        }
    });

    let captured = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(
        !captured.is_empty(),
        "expected at least one audit log line to be captured"
    );
    for secret in corpus {
        assert!(
            !captured.contains(secret),
            "audit log leaked original secret value: {secret}\ncaptured log:\n{captured}"
        );
    }
    assert!(
        captured.contains("\"event\":\"redaction\""),
        "expected redaction audit events in captured log:\n{captured}"
    );

    // P4-T3 DoD: 出力された各 JSON レコード (ネスト含む) に、値を運べる
    // フィールド名 (`value`/`original`/`text`/`body`) が存在しないことを確認する。
    const FORBIDDEN_KEYS: &[&str] = &["value", "original", "text", "body", "raw"];
    fn assert_no_forbidden_keys(v: &serde_json::Value, line: &str) {
        match v {
            serde_json::Value::Object(obj) => {
                for (key, val) in obj {
                    assert!(
                        !FORBIDDEN_KEYS.contains(&key.as_str()),
                        "audit log record contains a forbidden value-carrying field `{key}`: {line}"
                    );
                    assert_no_forbidden_keys(val, line);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    assert_no_forbidden_keys(item, line);
                }
            }
            _ => {}
        }
    }
    for line in captured.lines().filter(|l| !l.trim().is_empty()) {
        let record: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("audit log line is not valid JSON: {e}\nline: {line}"));
        assert_no_forbidden_keys(&record, line);
    }
}
