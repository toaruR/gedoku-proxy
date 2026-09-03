//! Field Map: type-tag ウォーカーによる「触ってよい場所」の限定
//! (設計 §7.3, §8.2 / 計画 P1-T3, P1-T4, P1-T5 / GAP-4, GAP-8, GAP-9 反映)。
//!
//! P1 の範囲では**検出も置換も行わない** (無検閲)。ここで提供するのは
//! 「スキャン対象リーフを漏れなく・禁止パスを一切含まずに列挙する」機構のみで、
//! P2 の検出エンジンがこの列挙結果を消費する。

use crate::pathspec::{resolve_top_level_policy, TopLevelPolicy, TOP_LEVEL_RULES};
use crate::pointer::{push_index, push_key};
use serde_json::Value;

/// 各フィールドに適用する方針。
#[derive(Debug, Clone, Copy)]
pub enum FieldPolicy {
    /// 無変更でそのまま転送する (既定。ここに列挙しないフィールドも暗黙にこれと同じ扱い)。
    AlwaysForward,
    /// 検出エンジン (P2) の対象。値の内部を書き換えうる。
    ScanAndSubstitute,
    /// 絶対に触らない (§7.3 禁止リスト)。
    ForbidTouch,
    /// スキャン対象ではなく、HMAC ハッシュ化などの専用処理に回す (例: `/metadata/user_id`)。
    /// jsonwalk はこの方針を持つフィールドを一切 walk しないため、
    /// スキャン対象として列挙されないことがそのまま保証される。
    HashOnly,
    /// 予約: ブロック単体では判定できない条件 (例: ツールの `strict` フラグ) による
    /// 条件付き方針。P2 でアダプタ層から実際の判定関数を配線する。
    ConditionalOn(fn(&Value) -> bool),
}

/// フィールド値の形。単純なリーフか、固定スキーマの入れ子オブジェクトか、
/// type タグ駆動で任意深さに再帰するブロック配列 (GAP-9) か、
/// 自由形式のネスト構造 (tool_use の `input` 等) か。
#[derive(Debug, Clone, Copy)]
pub enum FieldShape {
    Leaf(FieldPolicy),
    /// 固定スキーマの入れ子オブジェクト (例: image/document の `source`)。
    Object(&'static [FieldRule]),
    /// 文字列、または type タグを持つブロックの配列 (GAP-9: 任意深さに再帰)。
    ContentBlocks,
    /// 任意構造のネスト (オブジェクト/配列混在) で、内部の文字列リーフ全てに
    /// 同一方針を適用する (例: `tool_use.input/**`)。
    FreeformLeaves(FieldPolicy),
}

#[derive(Debug, Clone, Copy)]
pub struct FieldRule {
    pub name: &'static str,
    pub shape: FieldShape,
}

/// ブロック type タグ→対象フィールドの対応規則 (計画 §3.1 の境界型シグネチャ)。
pub trait BlockRule: Send + Sync {
    fn type_tag(&self) -> &'static str;
    fn leaf_fields(&self) -> &'static [FieldRule];
}

pub struct StaticBlockRule {
    pub type_tag: &'static str,
    pub fields: &'static [FieldRule],
}

impl BlockRule for StaticBlockRule {
    fn type_tag(&self) -> &'static str {
        self.type_tag
    }

    fn leaf_fields(&self) -> &'static [FieldRule] {
        self.fields
    }
}

const SOURCE_OBJECT_FIELDS: &[FieldRule] = &[
    // GAP-8: base64 画像/文書本体は既定禁止 (§7.3 `/…/source/data`)。
    FieldRule {
        name: "data",
        shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
    },
    // GAP-8: `type:"url"` のプリサインドURL/内部トークン付きURLは L1 スキャン対象として許可。
    FieldRule {
        name: "url",
        shape: FieldShape::Leaf(FieldPolicy::ScanAndSubstitute),
    },
];

/// §7.3 のブロック type タグ別規則。ここに列挙されない type タグ (未知の将来ブロック含む)
/// は GAP-9 の通りオープンリスト原則で丸ごと非対象・そのまま転送する。
pub const BLOCK_RULES: &[StaticBlockRule] = &[
    StaticBlockRule {
        type_tag: "text",
        fields: &[
            FieldRule {
                name: "text",
                shape: FieldShape::Leaf(FieldPolicy::ScanAndSubstitute),
            },
            FieldRule {
                name: "cache_control",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
        ],
    },
    StaticBlockRule {
        type_tag: "tool_use",
        fields: TOOL_USE_FIELDS,
    },
    StaticBlockRule {
        type_tag: "server_tool_use",
        fields: TOOL_USE_FIELDS,
    },
    StaticBlockRule {
        type_tag: "tool_result",
        fields: &[
            FieldRule {
                name: "content",
                shape: FieldShape::ContentBlocks,
            },
            FieldRule {
                name: "tool_use_id",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
            FieldRule {
                name: "cache_control",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
        ],
    },
    StaticBlockRule {
        type_tag: "thinking",
        fields: &[
            // §7.3: thinking は既定で触らない (⚠️ 条件付、有効化パスは P6-T4 で扱う)。
            FieldRule {
                name: "thinking",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
            FieldRule {
                name: "signature",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
            FieldRule {
                name: "cache_control",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
        ],
    },
    StaticBlockRule {
        type_tag: "redacted_thinking",
        fields: &[
            FieldRule {
                name: "data",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
            FieldRule {
                name: "cache_control",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
        ],
    },
    StaticBlockRule {
        type_tag: "image",
        fields: &[
            FieldRule {
                name: "source",
                shape: FieldShape::Object(SOURCE_OBJECT_FIELDS),
            },
            FieldRule {
                name: "cache_control",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
        ],
    },
    StaticBlockRule {
        type_tag: "document",
        fields: &[
            FieldRule {
                name: "source",
                shape: FieldShape::Object(SOURCE_OBJECT_FIELDS),
            },
            FieldRule {
                name: "cache_control",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
        ],
    },
    StaticBlockRule {
        type_tag: "search_result",
        fields: &[
            FieldRule {
                name: "content",
                shape: FieldShape::ContentBlocks,
            },
            FieldRule {
                name: "cache_control",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
        ],
    },
    StaticBlockRule {
        type_tag: "web_search_tool_result",
        fields: &[
            FieldRule {
                name: "content",
                shape: FieldShape::ContentBlocks,
            },
            FieldRule {
                name: "cache_control",
                shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
            },
        ],
    },
    StaticBlockRule {
        type_tag: "tool_reference",
        fields: &[],
    },
];

const TOOL_USE_FIELDS: &[FieldRule] = &[
    FieldRule {
        name: "input",
        shape: FieldShape::FreeformLeaves(FieldPolicy::ScanAndSubstitute),
    },
    FieldRule {
        name: "id",
        shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
    },
    FieldRule {
        name: "name",
        shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
    },
    FieldRule {
        name: "cache_control",
        shape: FieldShape::Leaf(FieldPolicy::ForbidTouch),
    },
];

fn block_rule_for(type_tag: &str) -> Option<&'static StaticBlockRule> {
    BLOCK_RULES.iter().find(|r| r.type_tag == type_tag)
}

/// L1/L2/L3 検出エンジン (P2) に渡す、スキャン対象リーフ文字列の JSON Pointer。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanTarget {
    pub pointer: String,
}

/// リクエスト/レスポンスボディ全体から、ScanAndSubstitute 対象のリーフを漏れなく列挙する。
/// 禁止パス (§7.3) は列挙対象のロジック自体が walk しないため構造的に含まれない。
///
/// GAP-4 (計画 P5-T3): トップレベルフィールドの可否は `pathspec::TOP_LEVEL_RULES`
/// (Exact JSON Pointer > 再帰glob `**` > 既定 AlwaysForward の優先順位) で判定する。
/// `system`/`messages` のみがブロック type-tag ウォーカーへ委譲され、その内部の
/// 可否判定は本モジュール冒頭の `BLOCK_RULES` が唯一の権威 (pathspec からは不可侵)。
pub fn collect_scan_targets(body: &Value) -> Vec<ScanTarget> {
    let mut out = Vec::new();
    let Some(obj) = body.as_object() else {
        return out;
    };

    for (key, val) in obj.iter() {
        let pointer = push_key("", key);
        match resolve_top_level_policy(TOP_LEVEL_RULES, &pointer) {
            TopLevelPolicy::RecurseSystemBlocks => walk_system(val, &mut out),
            TopLevelPolicy::RecurseMessages => {
                if let Some(items) = val.as_array() {
                    for (i, msg) in items.iter().enumerate() {
                        walk_message(msg, &push_index("/messages", i), &mut out);
                    }
                }
            }
            // AlwaysForward / ForbidTouch / HashOnly: 触れない。
            // 「触れない」ことがそのまま到達不能性の保証になる (§7.3 禁止リスト)。
            TopLevelPolicy::AlwaysForward
            | TopLevelPolicy::ForbidTouch
            | TopLevelPolicy::HashOnly => {}
        }
    }

    out
}

/// S7: `system` が配列の場合、先頭要素 (attribution block) は既定でスキャン対象外。
fn walk_system(system: &Value, out: &mut Vec<ScanTarget>) {
    match system {
        Value::String(_) => out.push(ScanTarget {
            pointer: "/system".to_string(),
        }),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                if i == 0 {
                    continue; // S7: attribution block
                }
                walk_typed_block(item, &push_index("/system", i), out);
            }
        }
        _ => {}
    }
}

/// S8: 会話途中の `role: "system"` エントリも含め、`content` の扱いは role によらず同一。
fn walk_message(msg: &Value, ptr: &str, out: &mut Vec<ScanTarget>) {
    let Some(content) = msg.get("content") else {
        return;
    };
    walk_content_blocks(content, &push_key(ptr, "content"), out);
}

fn walk_content_blocks(val: &Value, ptr: &str, out: &mut Vec<ScanTarget>) {
    match val {
        Value::String(_) => out.push(ScanTarget {
            pointer: ptr.to_string(),
        }),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk_typed_block(item, &push_index(ptr, i), out);
            }
        }
        _ => {}
    }
}

/// GAP-9: `type` タグを読み、登録済み規則に従って対象フィールドのみへ再帰する。
/// 未知の type タグ (将来追加されるブロック型含む) はオープンリスト原則により丸ごと非対象。
fn walk_typed_block(block: &Value, ptr: &str, out: &mut Vec<ScanTarget>) {
    let Some(obj) = block.as_object() else {
        return;
    };
    let Some(type_tag) = obj.get("type").and_then(Value::as_str) else {
        return;
    };
    let Some(rule) = block_rule_for(type_tag) else {
        return;
    };

    for field in rule.fields {
        let Some(field_val) = obj.get(field.name) else {
            continue;
        };
        apply_shape(&field.shape, field_val, &push_key(ptr, field.name), out);
    }
}

fn apply_shape(shape: &FieldShape, val: &Value, ptr: &str, out: &mut Vec<ScanTarget>) {
    match shape {
        FieldShape::Leaf(FieldPolicy::ScanAndSubstitute) => {
            if val.is_string() {
                out.push(ScanTarget {
                    pointer: ptr.to_string(),
                });
            }
        }
        FieldShape::Leaf(_) => {} // ForbidTouch / AlwaysForward / HashOnly / ConditionalOn: 触れない
        FieldShape::Object(fields) => {
            if let Some(obj) = val.as_object() {
                for f in *fields {
                    if let Some(v) = obj.get(f.name) {
                        apply_shape(&f.shape, v, &push_key(ptr, f.name), out);
                    }
                }
            }
        }
        FieldShape::ContentBlocks => walk_content_blocks(val, ptr, out),
        FieldShape::FreeformLeaves(FieldPolicy::ScanAndSubstitute) => walk_freeform(val, ptr, out),
        FieldShape::FreeformLeaves(_) => {}
    }
}

fn walk_freeform(val: &Value, ptr: &str, out: &mut Vec<ScanTarget>) {
    match val {
        Value::String(_) => out.push(ScanTarget {
            pointer: ptr.to_string(),
        }),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk_freeform(item, &push_index(ptr, i), out);
            }
        }
        Value::Object(map) => {
            for (k, v) in map.iter() {
                walk_freeform(v, &push_key(ptr, k), out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pointers(body: &Value) -> Vec<String> {
        let mut ps: Vec<String> = collect_scan_targets(body)
            .into_iter()
            .map(|t| t.pointer)
            .collect();
        ps.sort();
        ps
    }

    #[test]
    fn s7_excludes_attribution_block_but_scans_rest_of_system_array() {
        let body = json!({
            "system": [
                {"type": "text", "text": "attribution: claude-code v2.1.181"},
                {"type": "text", "text": "you are a helpful assistant"}
            ],
            "messages": []
        });
        assert_eq!(pointers(&body), vec!["/system/1/text"]);
    }

    #[test]
    fn system_as_plain_string_is_scanned_whole() {
        let body = json!({"system": "be helpful", "messages": []});
        assert_eq!(pointers(&body), vec!["/system"]);
    }

    #[test]
    fn s8_mid_conversation_role_system_is_scanned_like_any_other_message() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "system", "content": "reminder: be concise"}
            ]
        });
        assert_eq!(
            pointers(&body),
            vec!["/messages/0/content", "/messages/1/content"]
        );
    }

    #[test]
    fn gap9_recurses_into_tool_result_nested_search_result_at_arbitrary_depth() {
        let body = json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_secret_id_01",
                    "content": [{
                        "type": "search_result",
                        "content": [
                            {"type": "text", "text": "deeply nested leaf"}
                        ]
                    }]
                }]
            }]
        });
        assert_eq!(
            pointers(&body),
            vec!["/messages/0/content/0/content/0/content/0/text"]
        );
    }

    #[test]
    fn forbidden_paths_are_never_reachable_as_scan_targets() {
        let body = json!({
            "model": "claude-opus-5",
            "stop_sequences": ["STOP secret-token"],
            "cache_control": {"type": "ephemeral"},
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "output_config": {"format": "text secret"},
            "context_management": {"strategy": "secret"},
            "betas": ["secret-beta-flag"],
            "metadata": {"user_id": "user_secret_1234"},
            "tools": [{
                "name": "search_secret_tool",
                "input_schema": {"type": "object", "properties": {"q": {"type": "string"}}}
            }],
            "messages": [{
                "role": "assistant",
                "content": [
                    {
                        "type": "thinking",
                        "thinking": "let me think about the secret plan",
                        "signature": "sig-secret-abc"
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_secret_02",
                        "name": "search_secret_tool",
                        "cache_control": {"type": "ephemeral"},
                        "input": {"query": "public query text"}
                    },
                    {
                        "type": "image",
                        "source": {"type": "base64", "media_type": "image/png", "data": "iVBORwSECRETDATA"}
                    }
                ]
            }]
        });

        assert_eq!(pointers(&body), vec!["/messages/0/content/1/input/query"]);
    }

    #[test]
    fn gap8_source_url_is_scanned_but_source_data_never_is() {
        let body_url = json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "source": {"type": "url", "url": "https://internal.example/presigned?token=secret"}
                }]
            }]
        });
        assert_eq!(
            pointers(&body_url),
            vec!["/messages/0/content/0/source/url"]
        );

        let body_data = json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "source": {"type": "base64", "media_type": "image/png", "data": "SECRETBASE64"}
                }]
            }]
        });
        assert!(pointers(&body_data).is_empty());
    }

    #[test]
    fn unknown_block_type_is_skipped_entirely_open_list_principle() {
        let body = json!({
            "messages": [{
                "role": "assistant",
                "content": [{"type": "future_block_type_v99", "text": "should not be scanned"}]
            }]
        });
        assert!(pointers(&body).is_empty());
    }

    #[test]
    fn tool_use_freeform_input_scans_nested_string_leaves_only() {
        let body = json!({
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "run_query",
                    "input": {
                        "query": "select * from users",
                        "limit": 10,
                        "filters": ["active", "verified"],
                        "nested": {"note": "leaf under object"}
                    }
                }]
            }]
        });
        assert_eq!(
            pointers(&body),
            vec![
                "/messages/0/content/0/input/filters/0",
                "/messages/0/content/0/input/filters/1",
                "/messages/0/content/0/input/nested/note",
                "/messages/0/content/0/input/query",
            ]
        );
    }
}
