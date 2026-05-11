use evotengine::context::*;
use evotengine::types::*;
use proptest::prelude::*;

use super::fixtures::compaction_assert::*;
use super::fixtures::message_dsl::*;

fn arb_config_extreme() -> impl Strategy<Value = ContextConfig> {
    (
        prop_oneof![Just(0usize), Just(1), Just(10), 0..5000usize],
        prop_oneof![Just(0usize), Just(200), 0..200usize],
        prop_oneof![Just(0usize), Just(100), 0..20usize],
        prop_oneof![Just(0usize), Just(100), 0..20usize],
        prop_oneof![Just(0u8), Just(100), 0..100u8],
        prop_oneof![Just(0u8), Just(100), 0..100u8],
        prop_oneof![Just(0usize), Just(1), Just(3), 0..50usize],
    )
        .prop_map(
            |(max, sys, recent, first, trigger, target, max_msgs)| ContextConfig {
                max_context_tokens: max,
                system_prompt_tokens: sys,
                keep_recent: recent,
                keep_first: first,
                compact_trigger_pct: trigger,
                compact_target_pct: target,
                max_messages: max_msgs,
                ..Default::default()
            },
        )
}

fn arb_fuzz_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        "[ -~]{0,64}",
        (0usize..4, 0usize..512).prop_map(|(idx, len)| format!("line-{idx}-{}", "x".repeat(len))),
        prop::collection::vec("[ -~]{0,40}", 0..24).prop_map(|lines| lines.join("\n")),
        prop::collection::vec(
            prop_oneof![Just('中'), Just('🚀'), Just('\0'), Just('\n')],
            0..96
        )
        .prop_map(|chars| chars.into_iter().collect()),
    ]
}

fn arb_tool_id() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("dup".to_string()),
        "[a-zA-Z0-9_-]{1,24}",
    ]
}

fn arb_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        arb_fuzz_text().prop_map(|text| Content::Text { text }),
        arb_fuzz_text().prop_map(|thinking| Content::Thinking {
            thinking,
            signature: None,
        }),
        arb_tool_id().prop_map(|id| Content::ToolCall {
            id,
            name: "tool".into(),
            arguments: serde_json::json!({"arg": "x"}),
        }),
        ("[A-Za-z0-9+/=]{0,256}", prop::bool::ANY).prop_map(|(data, with_path)| {
            Content::Image {
                mime_type: "image/png".into(),
                source: ImageSource::Base64 {
                    data,
                    path: with_path.then(|| "/tmp/fuzz.png".into()),
                },
            }
        }),
    ]
}

fn arb_agent_message() -> impl Strategy<Value = AgentMessage> {
    prop_oneof![
        prop::collection::vec(arb_content(), 0..4).prop_map(|content| {
            AgentMessage::Llm(Message::User {
                content,
                timestamp: 0,
            })
        }),
        prop::collection::vec(arb_content(), 0..4).prop_map(|content| {
            AgentMessage::Llm(Message::Assistant {
                content,
                stop_reason: StopReason::Stop,
                model: "fuzz".into(),
                provider: "fuzz".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: None,
                response_id: None,
            })
        }),
        (
            arb_tool_id(),
            prop::collection::vec(arb_content(), 0..6),
            prop::bool::ANY,
            prop::bool::ANY,
        )
            .prop_map(|(tool_call_id, content, is_error, current_run)| {
                AgentMessage::Llm(Message::ToolResult {
                    tool_call_id,
                    tool_name: "tool".into(),
                    content,
                    is_error,
                    timestamp: 0,
                    retention: if current_run {
                        Retention::CurrentRun
                    } else {
                        Retention::Normal
                    },
                })
            }),
        arb_fuzz_text().prop_map(|text| {
            AgentMessage::Extension(ExtensionMessage::new(
                "fuzz",
                serde_json::json!({"text": text}),
            ))
        }),
    ]
}

fn arb_agent_messages() -> impl Strategy<Value = Vec<AgentMessage>> {
    prop::collection::vec(arb_agent_message(), 0..16)
}

fn arb_pattern() -> impl Strategy<Value = String> {
    prop::collection::vec(prop_oneof!["u", "a", "tr"], 1..15)
        .prop_map(|v| v.concat())
        .prop_filter("must contain at least one u", |s| s.contains('u'))
}

fn arb_sized_pattern_case() -> impl Strategy<Value = (String, usize, usize, ContextConfig)> {
    (
        arb_pattern(),
        10..200usize,
        10..500usize,
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
                max_messages: 50,
                ..Default::default()
            }),
    )
}

fn assert_compaction_invariants(
    input: &[AgentMessage],
    config: &ContextConfig,
    budget_state: &CompactionBudgetState,
    result: &CompactionResult,
) {
    assert_no_orphan_tool_pairs(&result.messages);
    assert_actions_match_level(result.stats.level, &result.stats.actions);
    assert!(result.stats.after_message_count <= result.stats.before_message_count);
    assert_eq!(result.stats.before_message_count, input.len());
    assert_eq!(result.stats.after_message_count, result.messages.len());
    assert!(result.stats.after_estimated_tokens <= result.stats.before_estimated_tokens);
    assert!(result.stats.after_estimated_tokens >= total_tokens(&result.messages));
    assert_eq!(
        result.stats.before_estimated_tokens,
        budget_state.estimated_tokens
    );

    for action in &result.stats.actions {
        assert!(action.index < input.len());
        if let Some(end) = action.end_index {
            assert!(end >= action.index);
            assert!(end < input.len());
        }
        assert!(action.after_tokens <= action.before_tokens);
    }

    let budget = config
        .max_context_tokens
        .saturating_sub(config.system_prompt_tokens);
    if result.stats.before_estimated_tokens <= budget {
        assert!(result.stats.after_estimated_tokens <= budget);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn compact_fuzz_arbitrary_messages_keep_core_invariants(
        messages in arb_agent_messages(),
        config in arb_config_extreme(),
        provider_extra in prop_oneof![Just(0usize), 0..20_000usize],
    ) {
        let budget_state = CompactionBudgetState {
            estimated_tokens: total_tokens(&messages).saturating_add(provider_extra),
        };
        let result = compact_messages(messages.clone(), &config, &budget_state);
        assert_compaction_invariants(&messages, &config, &budget_state, &result);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn compact_fuzz_pattern_transcripts_preserve_tool_pair_integrity(
        (pattern, pad, tool_out, config) in arb_sized_pattern_case(),
    ) {
        let messages = pat(&pattern).pad(pad).tool_output(tool_out).build();
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let result = compact_messages(messages, &config, &budget_state);
        assert_no_orphan_tool_pairs(&result.messages);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn compact_fuzz_second_pass_does_not_reintroduce_invalid_pairs(
        messages in arb_agent_messages(),
        config in arb_config_extreme(),
    ) {
        let budget_state = CompactionBudgetState::from_messages(&messages);
        let r1 = compact_messages(messages, &config, &budget_state);
        let budget_state2 = CompactionBudgetState {
            estimated_tokens: r1.stats.after_estimated_tokens,
        };
        let r2 = compact_messages(r1.messages.clone(), &config, &budget_state2);
        assert_no_orphan_tool_pairs(&r2.messages);
        prop_assert!(r2.messages.len() <= r1.messages.len());
    }
}
