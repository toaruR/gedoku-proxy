//! パスパターン文法の統一実装 (設計 §7.3, §18.2 / GAP-4 反映 / 計画 P5-T3)。
//!
//! GAP-4 の解決方針: 「固定深度パス＝JSON Pointer完全一致、可変深度＝`**`再帰glob、
//! ブロック内部＝type-tagウォーカー規則が優先」という3層の適用順序を型で表現する。
//! ここで扱うのはトップレベル (system/messages と同階層) のフィールドのみで、
//! ブロック内部 (`fieldmap::BLOCK_RULES`) の判定はこのモジュールを経由しない
//! (=常に type-tag ウォーカー規則が優先される、という GAP-4 の記述をそのまま構造で保証する)。

/// トップレベルフィールド1件に対する方針。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLevelPolicy {
    /// 無変更で転送 (既定、ここに列挙しないフィールドも暗黙にこれと同じ扱い)。
    AlwaysForward,
    /// 絶対に触らない。
    ForbidTouch,
    /// スキャン対象ではなく専用処理 (HMACハッシュ化等) に回す。
    HashOnly,
    /// `system` フィールドとして type-tag ウォーカーへ委譲する。
    RecurseSystemBlocks,
    /// `messages` フィールドとして type-tag ウォーカーへ委譲する。
    RecurseMessages,
}

/// パスの表記文法。固定深度 = JSON Pointer 完全一致、可変深度 = 末尾 `/**` の再帰glob。
#[derive(Debug, Clone, Copy)]
pub enum PathSpec {
    /// JSON Pointer 完全一致 (例: `/model`)。
    Exact(&'static str),
    /// 末尾 `/**` の再帰glob (例: `/tools/**` は `/tools` 自身と配下すべてに一致)。
    Glob(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct TopLevelRule {
    pub pattern: PathSpec,
    pub policy: TopLevelPolicy,
}

fn glob_matches(glob: &str, pointer: &str) -> bool {
    let Some(prefix) = glob.strip_suffix("/**") else {
        return glob == pointer;
    };
    pointer == prefix || pointer.starts_with(&format!("{prefix}/"))
}

/// 優先順位 (GAP-4): Exact 一致 > Glob 一致 > 既定 (`AlwaysForward`)。
/// 同一階層で Exact と Glob が競合した場合、より特定的な Exact が勝つ。
pub fn resolve_top_level_policy(rules: &[TopLevelRule], pointer: &str) -> TopLevelPolicy {
    for rule in rules {
        if let PathSpec::Exact(p) = rule.pattern {
            if p == pointer {
                return rule.policy;
            }
        }
    }
    for rule in rules {
        if let PathSpec::Glob(g) = rule.pattern {
            if glob_matches(g, pointer) {
                return rule.policy;
            }
        }
    }
    TopLevelPolicy::AlwaysForward
}

/// §7.3 のトップレベル規則表。`collect_scan_targets` はここに従い、`system`/`messages`
/// のみをブロック type-tag ウォーカーへ委譲する。他は列挙対象外 (=到達不能) にする。
pub const TOP_LEVEL_RULES: &[TopLevelRule] = &[
    TopLevelRule {
        pattern: PathSpec::Exact("/system"),
        policy: TopLevelPolicy::RecurseSystemBlocks,
    },
    TopLevelRule {
        pattern: PathSpec::Exact("/messages"),
        policy: TopLevelPolicy::RecurseMessages,
    },
    TopLevelRule {
        pattern: PathSpec::Exact("/metadata/user_id"),
        policy: TopLevelPolicy::HashOnly,
    },
    TopLevelRule {
        pattern: PathSpec::Glob("/stop_sequences/**"),
        policy: TopLevelPolicy::ForbidTouch,
    },
    TopLevelRule {
        pattern: PathSpec::Glob("/tools/**"),
        policy: TopLevelPolicy::ForbidTouch,
    },
    TopLevelRule {
        pattern: PathSpec::Glob("/cache_control/**"),
        policy: TopLevelPolicy::ForbidTouch,
    },
    TopLevelRule {
        pattern: PathSpec::Glob("/thinking/**"),
        policy: TopLevelPolicy::ForbidTouch,
    },
    TopLevelRule {
        pattern: PathSpec::Glob("/output_config/**"),
        policy: TopLevelPolicy::ForbidTouch,
    },
    TopLevelRule {
        pattern: PathSpec::Glob("/context_management/**"),
        policy: TopLevelPolicy::ForbidTouch,
    },
    TopLevelRule {
        pattern: PathSpec::Glob("/betas/**"),
        policy: TopLevelPolicy::ForbidTouch,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_beats_glob_when_both_match() {
        let rules = &[
            TopLevelRule {
                pattern: PathSpec::Glob("/tools/**"),
                policy: TopLevelPolicy::ForbidTouch,
            },
            TopLevelRule {
                pattern: PathSpec::Exact("/tools/0/name"),
                policy: TopLevelPolicy::AlwaysForward,
            },
        ];
        assert_eq!(
            resolve_top_level_policy(rules, "/tools/0/name"),
            TopLevelPolicy::AlwaysForward
        );
        assert_eq!(
            resolve_top_level_policy(rules, "/tools/0/input_schema"),
            TopLevelPolicy::ForbidTouch
        );
    }

    #[test]
    fn unmatched_pointer_defaults_to_always_forward() {
        assert_eq!(
            resolve_top_level_policy(TOP_LEVEL_RULES, "/temperature"),
            TopLevelPolicy::AlwaysForward
        );
    }

    #[test]
    fn glob_matches_prefix_itself_and_descendants() {
        assert!(glob_matches("/tools/**", "/tools"));
        assert!(glob_matches("/tools/**", "/tools/0/name"));
        assert!(!glob_matches("/tools/**", "/toolset"));
    }

    #[test]
    fn production_top_level_rules_route_system_and_messages_to_block_walker() {
        assert_eq!(
            resolve_top_level_policy(TOP_LEVEL_RULES, "/system"),
            TopLevelPolicy::RecurseSystemBlocks
        );
        assert_eq!(
            resolve_top_level_policy(TOP_LEVEL_RULES, "/messages"),
            TopLevelPolicy::RecurseMessages
        );
        assert_eq!(
            resolve_top_level_policy(TOP_LEVEL_RULES, "/tools"),
            TopLevelPolicy::ForbidTouch
        );
        assert_eq!(
            resolve_top_level_policy(TOP_LEVEL_RULES, "/metadata/user_id"),
            TopLevelPolicy::HashOnly
        );
    }
}
