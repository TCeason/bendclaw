//! DSL-style structural tests for compaction.

use evotengine::context::compaction::config::CompactionConfig;
use evotengine::context::compaction::executor;
use evotengine::context::compaction::executor::ExecutionResult;
use evotengine::context::compaction::summary::LlmPolicy;
use evotengine::context::SummarizerMode;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;

use super::fixtures::compaction_assert::*;
use super::fixtures::message_dsl::*;

fn config_for_dsl() -> CompactionConfig {
    CompactionConfig {
        context_window: 10_000,
        reserve_tokens: 2_000,
        advertised_context_window: None,
        trigger_tokens: None,
        keep_recent_tokens: 500,
        summarizer_mode: SummarizerMode::default(),
        summary_max_bytes: 4000,
    }
}

async fn compact_pattern_with_size(
    pattern: &str,
    pad_chars: usize,
    tool_output_chars: usize,
) -> Vec<AgentMessage> {
    let config = config_for_dsl();
    let messages = pat(pattern)
        .pad(pad_chars)
        .tool_output(tool_output_chars)
        .build();
    compact(messages, &config, pattern).await
}

async fn compact(
    messages: Vec<AgentMessage>,
    config: &CompactionConfig,
    pattern: &str,
) -> Vec<AgentMessage> {
    let plan = match evotengine::plan_messages(&messages, config.keep_recent_tokens) {
        Some(plan) => plan,
        None => panic!("expected compaction plan for pattern {pattern}"),
    };
    match executor::execute(
        messages,
        &plan,
        config,
        None,
        None,
        executor::ExecutionOptions {
            llm_policy: LlmPolicy::Required,
            observer: None,
            cancel: CancellationToken::new(),
        },
    )
    .await
    {
        ExecutionResult::Compacted(outcome) => outcome.messages,
        ExecutionResult::Skipped(_) => panic!("expected compaction for pattern {pattern}"),
    }
}

async fn compact_pattern(pattern: &str) -> Vec<AgentMessage> {
    compact_pattern_with_size(pattern, 300, 1200).await
}

#[tokio::test]
async fn dsl_compaction_preserves_valid_tool_pairs() {
    let result = compact_pattern("u tr tr tr tr tr u a").await;

    assert_no_orphan_tool_pairs(&result);
    assert_eq!(count_user_markers(&result), 1);
}

#[tokio::test]
async fn dsl_compaction_can_split_current_turn_without_orphans() {
    let result = compact_pattern("u a u tr tr tr tr tr tr a").await;

    assert_no_orphan_tool_pairs(&result);
    assert_eq!(count_user_markers(&result), 1);
}

#[tokio::test]
async fn dsl_compaction_removes_orphan_tool_call_created_by_boundary() {
    let config = CompactionConfig { ..config_for_dsl() };
    let messages = pat("u tr u tr tr tr tr u")
        .pad(300)
        .tool_output(1200)
        .build();
    let result = compact(messages, &config, "u tr u tr tr tr tr u").await;

    assert_no_orphan_tool_pairs(&result);
    assert_eq!(count_user_markers(&result), 1);
}

#[tokio::test]
async fn dsl_compaction_shape_is_marker_then_tail() {
    let result = compact_pattern_with_size("u a u a u a u a u a u a", 1000, 10).await;

    assert_no_orphan_tool_pairs(&result);
    assert_eq!(count_user_markers(&result), 1);

    let marker_index = match result.iter().position(|msg| {
        matches!(msg, AgentMessage::Llm(Message::User { content, .. }) if content.iter().any(|c| matches!(c, Content::Text { text } if text.contains("[Context compacted"))))
    }) {
        Some(index) => index,
        None => panic!("expected compaction marker"),
    };

    assert_eq!(
        marker_index, 0,
        "summary marker should replace all evicted history"
    );
    assert!(result.len() >= 3, "tail should retain recent messages");
}

#[test]
fn dsl_sanitize_removes_orphan_calls_and_results() {
    let messages = pat("u T u tr T u").build();
    let result = evotengine::sanitize_tool_pairs(messages);

    assert_no_orphan_tool_pairs(&result);
    assert_pattern(&result, "u u tr u");
}
