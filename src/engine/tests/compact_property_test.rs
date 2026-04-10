mod helpers;

use bendengine::context::*;
use helpers::message_pattern::*;
use proptest::prelude::*;

/// Generate a random valid pattern from atomic units: "u", "a", "tr"
fn arb_pattern() -> impl Strategy<Value = String> {
    prop::collection::vec(prop_oneof!["u", "a", "tr"], 1..15)
        .prop_map(|v| v.concat())
        .prop_filter("must contain at least one u", |s| s.contains('u'))
}

/// Generate a random ContextConfig with ranges that cover all levels
fn arb_config() -> impl Strategy<Value = ContextConfig> {
    (
        100..5000usize,
        0..100usize,
        1..8usize,
        0..3usize,
        8..50usize,
    )
        .prop_map(|(max, sys, recent, first, max_lines)| ContextConfig {
            max_context_tokens: max,
            system_prompt_tokens: sys,
            keep_recent: recent,
            keep_first: first,
            tool_output_max_lines: max_lines,
        })
}

// ---------------------------------------------------------------------------
// P1: compact never produces orphan tool_call / tool_result
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_preserves_tool_pair_integrity(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let result = compact_messages(messages, &config);
        assert_no_orphan_tool_pairs(&result.messages);
    }
}

// ---------------------------------------------------------------------------
// P2: level 0 means messages are unchanged
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_level_zero_is_identity(
        pattern in arb_pattern(),
    ) {
        // Use a very large budget so compaction is never needed
        let messages = pat(&pattern).pad(10).tool_output(10).build();
        let config = ContextConfig {
            max_context_tokens: 500_000,
            system_prompt_tokens: 0,
            keep_recent: 100,
            keep_first: 100,
            tool_output_max_lines: 1000,
        };
        let original_len = messages.len();
        let result = compact_messages(messages, &config);
        prop_assert_eq!(result.stats.level, 0);
        prop_assert!(result.stats.actions.is_empty());
        prop_assert_eq!(result.messages.len(), original_len);
    }
}

// ---------------------------------------------------------------------------
// P3: actions method matches the reported level
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_actions_match_level(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let result = compact_messages(messages, &config);
        let level = result.stats.level;
        if level == 0 {
            prop_assert!(result.stats.actions.is_empty());
        } else {
            assert_actions_match_level(level, &result.stats.actions);
        }
    }
}

// ---------------------------------------------------------------------------
// P4: level <= 2 respects budget
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn compact_respects_budget_before_level3(
        pattern in arb_pattern(),
        pad in 10..3000usize,
        tool_out in 10..5000usize,
        config in arb_config(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let result = compact_messages(messages, &config);
        if result.stats.level > 0 && result.stats.level <= 2 {
            let budget = config.max_context_tokens.saturating_sub(config.system_prompt_tokens);
            prop_assert!(
                result.stats.after_estimated_tokens <= budget,
                "level {} should respect budget: after={} > budget={}",
                result.stats.level,
                result.stats.after_estimated_tokens,
                budget,
            );
        }
    }
}
