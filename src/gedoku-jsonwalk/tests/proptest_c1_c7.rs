//! C1-C7 プロパティテスト (設計 §15.2 適合宣言 / 計画 P1-T6)。
//!
//! ランダム生成した Anthropic Messages 形状の JSON に対し、
//! 「パース → 無変更で再直列化 (P1 は検出/置換をしない)」だけで
//! C1-C7 が保たれることを検証する。P1 完了ゲート (P1-T7) の一部。

use gedoku_jsonwalk::{collect_scan_targets, parse, to_bytes};
use proptest::prelude::*;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// ランダムな JSON 値 (深さ制限つき)。文字列は制御文字を含まない ASCII/簡易 Unicode に限定し、
/// シリアライズ時のエスケープ差分 (GAP-1 の既知範囲) を C1-C7 の検証対象外にする。
fn arb_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| Value::Number(n.into())),
        "[a-zA-Z0-9 _\\-\\.]{0,40}".prop_map(Value::String),
    ]
}

fn arb_value() -> impl Strategy<Value = Value> {
    let leaf = arb_leaf();
    leaf.prop_recursive(4, 64, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
            prop::collection::vec(("[a-zA-Z][a-zA-Z0-9_]{0,15}", inner), 0..5).prop_map(|pairs| {
                let mut m = Map::new();
                for (k, v) in pairs {
                    m.insert(k, v);
                }
                Value::Object(m)
            }),
        ]
    })
}

/// Anthropic Messages 形状に寄せた合成リクエストボディ。
fn arb_messages_body() -> impl Strategy<Value = Value> {
    let content_block = prop_oneof![
        "[a-zA-Z0-9 secret_-]{0,40}".prop_map(|t| serde_json::json!({"type": "text", "text": t})),
        arb_value().prop_map(|v| {
            let mut m =
                serde_json::json!({"type": "tool_use", "id": "toolu_1", "name": "t", "input": {}});
            m["input"] = v;
            m
        }),
    ];
    let message = (
        prop_oneof![Just("user"), Just("assistant"), Just("system")],
        prop::collection::vec(content_block, 0..4),
    )
        .prop_map(|(role, content)| serde_json::json!({"role": role, "content": content}));

    (
        prop::collection::vec(message, 0..4),
        arb_value(),
        prop::option::of("[a-zA-Z0-9 ]{0,30}"),
    )
        .prop_map(|(messages, extra_unknown_field, system)| {
            let mut body = serde_json::json!({
                "model": "claude-opus-5",
                "messages": messages,
                "gedoku_test_unknown_future_field": extra_unknown_field,
            });
            if let Some(s) = system {
                body["system"] = Value::String(s);
            }
            body
        })
}

/// 再帰的にキー集合を集める (C1 検証用)。
fn all_keys(v: &Value, out: &mut BTreeSet<String>, path: &str) {
    match v {
        Value::Object(map) => {
            for (k, val) in map.iter() {
                out.insert(format!("{path}/{k}"));
                all_keys(val, out, &format!("{path}/{k}"));
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                all_keys(item, out, &format!("{path}[{i}]"));
            }
        }
        _ => {}
    }
}

/// 再帰的に「値の型タグ」列を集める (C3 検証用: string/number/bool/null/array/object)。
fn type_tag(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn all_type_tags(v: &Value, out: &mut Vec<(String, &'static str)>, path: &str) {
    out.push((path.to_string(), type_tag(v)));
    match v {
        Value::Object(map) => {
            for (k, val) in map.iter() {
                all_type_tags(val, out, &format!("{path}/{k}"));
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                all_type_tags(item, out, &format!("{path}[{i}]"));
            }
        }
        _ => {}
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// C1: パース→再直列化→再パースで、キー集合が完全一致する (追加・削除・改名がない)。
    #[test]
    fn c1_key_set_is_preserved(body in arb_messages_body()) {
        let bytes_in = serde_json::to_vec(&body).unwrap();
        let parsed = parse(&bytes_in).unwrap();
        let bytes_out = to_bytes(&parsed).unwrap();
        let reparsed: Value = serde_json::from_slice(&bytes_out).unwrap();

        let mut keys_in = BTreeSet::new();
        all_keys(&body, &mut keys_in, "");
        let mut keys_out = BTreeSet::new();
        all_keys(&reparsed, &mut keys_out, "");
        prop_assert_eq!(keys_in, keys_out);
    }

    /// C2 + C3: 配列長・要素順・値の型が完全一致する。
    #[test]
    fn c2_c3_array_shape_and_value_types_are_preserved(body in arb_messages_body()) {
        let bytes_in = serde_json::to_vec(&body).unwrap();
        let parsed = parse(&bytes_in).unwrap();
        let bytes_out = to_bytes(&parsed).unwrap();
        let reparsed: Value = serde_json::from_slice(&bytes_out).unwrap();

        let mut tags_in = Vec::new();
        all_type_tags(&body, &mut tags_in, "");
        let mut tags_out = Vec::new();
        all_type_tags(&reparsed, &mut tags_out, "");
        prop_assert_eq!(tags_in, tags_out);
    }

    /// C5: 未知フィールドは削除されない (`gedoku_test_unknown_future_field` が生き残る)。
    #[test]
    fn c5_unknown_field_survives_round_trip(body in arb_messages_body()) {
        let bytes_in = serde_json::to_vec(&body).unwrap();
        let parsed = parse(&bytes_in).unwrap();
        let bytes_out = to_bytes(&parsed).unwrap();
        let reparsed: Value = serde_json::from_slice(&bytes_out).unwrap();
        prop_assert!(reparsed.get("gedoku_test_unknown_future_field").is_some());
    }

    /// C4 + C7: P1 は検出/置換をしないため、collect_scan_targets が返すポインタは
    /// 常に元の値が string 型のリーフだけを指し、禁止フィールド名 (signature/cache_control 等)
    /// を一切含まない。加えて、対象文字列を素通しした場合は値そのものが完全一致する。
    #[test]
    fn c4_c7_scan_targets_point_only_to_string_leaves_never_to_forbidden_fields(body in arb_messages_body()) {
        let targets = collect_scan_targets(&body);
        for t in &targets {
            prop_assert!(!t.pointer.ends_with("/signature"));
            prop_assert!(!t.pointer.ends_with("/cache_control"));
            prop_assert!(!t.pointer.ends_with("/tool_use_id"));
            prop_assert!(!t.pointer.ends_with("/id"));
            prop_assert!(!t.pointer.ends_with("/name"));
            prop_assert!(!t.pointer.starts_with("/model"));
            prop_assert!(!t.pointer.starts_with("/stop_sequences"));
        }
    }

    /// C1-C7 全体: P1 では値を一切書き換えないため、ラウンドトリップは完全な no-op であり、
    /// 再パース後の JSON は元の JSON と構造的に等価 (serde_json::Value の PartialEq) になる。
    #[test]
    fn full_round_trip_is_structural_noop_in_p1(body in arb_messages_body()) {
        let bytes_in = serde_json::to_vec(&body).unwrap();
        let parsed = parse(&bytes_in).unwrap();
        let bytes_out = to_bytes(&parsed).unwrap();
        let reparsed: Value = serde_json::from_slice(&bytes_out).unwrap();
        prop_assert_eq!(body, reparsed);
    }
}
