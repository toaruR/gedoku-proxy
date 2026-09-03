//! P1 完了判定ハーネス (計画 P1-T7)。
//!
//! 計画の DoD は「実際の Claude Code セッションから採取した HAR ファイルを
//! insta golden としてリプレイし、差分ゼロを証明する」だが、本リポジトリには
//! 実セッションの HAR キャプチャが存在しない (取得は運用側のタスクであり
//! P1 実装単独では用意できない)。そのため本ハーネスは **代替として**、
//! 設計 §7.3/§8 の各構造 (attribution block, mid-conversation role:system,
//! GAP-9 の入れ子ブロック, thinking/signature, image source) を再現した
//! 合成フィクスチャに対し、P1 パイプライン (parse → collect_scan_targets →
//! 無変更で to_bytes) が「無検閲のままバイト同一性を保つ」ことを直接検証する。
//! 実 HAR によるリプレイは §5.1 の未了事項として引き続き残す。

use gedoku_jsonwalk::{collect_scan_targets, parse, to_bytes};
use serde_json::{json, Value};

/// 「素通し (直結相当)」= canonical にシリアライズしたバイト列を、
/// gedoku-jsonwalk の parse→to_bytes に通した結果と比較する。
fn assert_uncensored_byte_identity(body: &Value) -> Vec<u8> {
    let canonical_in = serde_json::to_vec(body).expect("canonical serialize");
    let parsed = parse(&canonical_in).expect("parse");
    let out = to_bytes(&parsed).expect("to_bytes");
    assert_eq!(
        out, canonical_in,
        "P1 (無検閲) では parse→to_bytes は no-op でなければならない"
    );
    out
}

#[test]
fn harness_basic_conversation_system_string() {
    let body = json!({
        "model": "claude-opus-5",
        "system": "You are a helpful assistant.",
        "messages": [
            {"role": "user", "content": "hello, my email is alice@example.com"},
            {"role": "assistant", "content": "Hi! How can I help?"}
        ],
        "max_tokens": 1024
    });
    assert_uncensored_byte_identity(&body);
}

#[test]
fn harness_system_array_attribution_and_mid_conversation_role_system() {
    let body = json!({
        "model": "claude-opus-5",
        "system": [
            {"type": "text", "text": "claude-code/2.1.181 fingerprint=abc123"},
            {"type": "text", "text": "You are operating in a sandbox.", "cache_control": {"type": "ephemeral"}}
        ],
        "messages": [
            {"role": "user", "content": "run the tests"},
            {"role": "system", "content": "reminder: do not leak sk-ant-SEEDVALUE00000000000000"},
            {"role": "assistant", "content": "Running tests now."}
        ]
    });
    let out = assert_uncensored_byte_identity(&body);
    let reparsed: Value = serde_json::from_slice(&out).unwrap();

    // S6/S7/S8: attribution block (system[0]) と cache_control は無変更、
    // 会話途中の role:"system" も他メッセージと同様に構造保存される。
    assert_eq!(reparsed["system"][0], body["system"][0]);
    assert_eq!(
        reparsed["system"][1]["cache_control"],
        body["system"][1]["cache_control"]
    );
    assert_eq!(reparsed["messages"][1]["role"], "system");
    assert_eq!(
        reparsed["messages"][1]["content"],
        body["messages"][1]["content"]
    );

    let targets = collect_scan_targets(&reparsed);
    let pointers: Vec<&str> = targets.iter().map(|t| t.pointer.as_str()).collect();
    assert!(pointers.contains(&"/system/1/text"));
    assert!(
        !pointers.contains(&"/system/0/text"),
        "attribution block は非対象 (S7)"
    );
    assert!(
        pointers.contains(&"/messages/1/content"),
        "mid-conversation role:system も対象 (S8)"
    );
}

#[test]
fn harness_gap9_nested_tool_result_search_result_and_gap8_image_sources() {
    let body = json!({
        "model": "claude-opus-5",
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": "toolu_do_not_touch_01",
                    "content": [{
                        "type": "search_result",
                        "content": [
                            {"type": "text", "text": "result snippet containing user@example.com"}
                        ]
                    }]
                },
                {
                    "type": "image",
                    "source": {"type": "base64", "media_type": "image/png", "data": "iVBORDONOTTOUCH=="}
                },
                {
                    "type": "image",
                    "source": {"type": "url", "url": "https://internal.example/blob?token=SCANME"}
                }
            ]
        }]
    });
    let out = assert_uncensored_byte_identity(&body);
    let reparsed: Value = serde_json::from_slice(&out).unwrap();

    // 禁止フィールドはバイト単位で無変更 (tool_use_id, source.data)。
    assert_eq!(
        reparsed["messages"][0]["content"][0]["tool_use_id"],
        body["messages"][0]["content"][0]["tool_use_id"]
    );
    assert_eq!(
        reparsed["messages"][0]["content"][1]["source"]["data"],
        body["messages"][0]["content"][1]["source"]["data"]
    );

    let targets = collect_scan_targets(&reparsed);
    let pointers: Vec<&str> = targets.iter().map(|t| t.pointer.as_str()).collect();
    assert_eq!(
        pointers,
        vec![
            "/messages/0/content/0/content/0/content/0/text",
            "/messages/0/content/2/source/url",
        ]
    );
}

#[test]
fn harness_thinking_signature_and_forbidden_top_level_fields_untouched() {
    let body = json!({
        "model": "claude-opus-5",
        "stop_sequences": ["STOP-secret-marker"],
        "metadata": {"user_id": "user_do_not_scan_as_plain_text"},
        "messages": [{
            "role": "assistant",
            "content": [
                {
                    "type": "thinking",
                    "thinking": "internal reasoning that must not be touched",
                    "signature": "sig-do-not-touch-xyz"
                },
                {"type": "text", "text": "Here is the answer, contact bob@example.com for details."}
            ]
        }]
    });
    let out = assert_uncensored_byte_identity(&body);
    let reparsed: Value = serde_json::from_slice(&out).unwrap();

    assert_eq!(reparsed["stop_sequences"], body["stop_sequences"]);
    assert_eq!(reparsed["metadata"], body["metadata"]);
    assert_eq!(
        reparsed["messages"][0]["content"][0]["thinking"],
        body["messages"][0]["content"][0]["thinking"]
    );
    assert_eq!(
        reparsed["messages"][0]["content"][0]["signature"],
        body["messages"][0]["content"][0]["signature"]
    );

    let targets = collect_scan_targets(&reparsed);
    let pointers: Vec<&str> = targets.iter().map(|t| t.pointer.as_str()).collect();
    assert_eq!(pointers, vec!["/messages/0/content/1/text"]);
}

#[test]
fn harness_unknown_top_level_and_unknown_block_type_survive_untouched() {
    let body = json!({
        "model": "claude-opus-5",
        "messages": [{
            "role": "user",
            "content": [{"type": "future_block_kind_v99", "payload": "opaque, must pass through"}]
        }],
        "gedoku_future_top_level_field": {"anything": ["at", "all"]}
    });
    let out = assert_uncensored_byte_identity(&body);
    let reparsed: Value = serde_json::from_slice(&out).unwrap();

    assert_eq!(
        reparsed["gedoku_future_top_level_field"],
        body["gedoku_future_top_level_field"]
    );
    assert_eq!(
        reparsed["messages"][0]["content"][0],
        body["messages"][0]["content"][0]
    );
    assert!(collect_scan_targets(&reparsed).is_empty());
}
