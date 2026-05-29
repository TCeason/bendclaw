//! Tests for the executor module.

use evotengine::context::compaction::config::CompactionConfig;
use evotengine::context::compaction::executor;
use evotengine::context::compaction::planner;
use evotengine::context::SummarizerMode;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;

fn user_msg(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        timestamp: 0,
    })
}

fn assistant_msg(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn tool_call_msg(id: &str, name: &str, path: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: serde_json::json!({"path": path}),
        }],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn tool_result_msg(id: &str, text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: id.to_string(),
        tool_name: "read".to_string(),
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        is_error: false,
        timestamp: 0,
        retention: Retention::default(),
    })
}

fn big_text(n: usize) -> String {
    "x".repeat(n)
}

fn config_small() -> CompactionConfig {
    CompactionConfig {
        context_window: 10_000,
        reserve_tokens: 2_000,
        keep_recent_tokens: 1_000,
        keep_recent_min: 2,
        keep_first: 2,
        max_tool_result_tokens: 500,
        tool_output_max_lines: 50,
        keep_recent_images: 1,
        summarizer_mode: SummarizerMode::default(),
        summary_max_chars: 4000,
    }
}

fn planned(
    messages: &[AgentMessage],
    config: &CompactionConfig,
) -> evotengine::context::compaction::types::CompactionPlan {
    match planner::plan(messages, config) {
        Some(plan) => plan,
        None => panic!("expected compaction plan"),
    }
}

#[tokio::test]
async fn executor_reduces_message_count() {
    let config = config_small();
    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("recent answer"));

    let plan = planned(&messages, &config);
    let original_count = messages.len();

    let cancel = CancellationToken::new();
    let outcome = executor::execute(messages, &plan, &config, None, None, cancel).await;

    assert!(outcome.messages.len() < original_count);
    assert!(outcome.stats.messages_evicted > 0);
    assert!(outcome.stats.before_message_count == original_count);
    assert!(outcome.stats.after_message_count == outcome.messages.len());
}

#[tokio::test]
async fn executor_preserves_pinned_head() {
    let config = config_small();
    let first_user = user_msg("first user message");
    let first_assistant = assistant_msg("first assistant message");
    let mut messages = vec![first_user.clone(), first_assistant.clone()];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("recent answer"));

    let plan = planned(&messages, &config);
    let cancel = CancellationToken::new();
    let outcome = executor::execute(messages, &plan, &config, None, None, cancel).await;

    // First two messages should be preserved
    let first_text = match &outcome.messages[0] {
        AgentMessage::Llm(Message::User { content, .. }) => content
            .iter()
            .filter_map(|c| match c {
                Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>(),
        _ => String::new(),
    };
    assert_eq!(first_text, "first user message");

    let second_text = match &outcome.messages[1] {
        AgentMessage::Llm(Message::Assistant { content, .. }) => content
            .iter()
            .filter_map(|c| match c {
                Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>(),
        _ => String::new(),
    };
    assert_eq!(second_text, "first assistant message");
}

#[tokio::test]
async fn executor_inserts_marker() {
    let config = config_small();
    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("recent answer"));

    let plan = planned(&messages, &config);
    let cancel = CancellationToken::new();
    let outcome = executor::execute(messages, &plan, &config, None, None, cancel).await;

    // There should be a marker message containing "[Context compacted"
    let has_marker = outcome.messages.iter().any(|m| {
        if let AgentMessage::Llm(Message::User { content, .. }) = m {
            content.iter().any(|c| match c {
                Content::Text { text } => text.contains("[Context compacted"),
                _ => false,
            })
        } else {
            false
        }
    });
    assert!(has_marker, "Expected a compaction marker message");
}

#[tokio::test]
async fn executor_tracks_file_ops_in_state() {
    let config = config_small();
    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    // Add tool calls with file paths in the evict zone
    for i in 0..10 {
        messages.push(user_msg(&big_text(300)));
        messages.push(tool_call_msg(
            &format!("tc{i}"),
            "edit",
            &format!("src/file{i}.rs"),
        ));
        messages.push(tool_result_msg(&format!("tc{i}"), &big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("recent answer"));

    let plan = planned(&messages, &config);
    let cancel = CancellationToken::new();
    let outcome = executor::execute(messages, &plan, &config, None, None, cancel).await;

    // State should have tracked file edits
    assert!(!outcome.state.file_ops.edited.is_empty());
}

#[test]
fn sanitize_tool_pairs_removes_orphans() {
    let messages = vec![
        user_msg("hello"),
        tool_call_msg("tc1", "read", "foo.rs"),
        // No matching tool result for tc1
        user_msg("next"),
    ];

    let result = evotengine::sanitize_tool_pairs(messages);
    // The orphan tool call should be removed
    let has_tool_call = result.iter().any(|m| {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = m {
            content
                .iter()
                .any(|c| matches!(c, Content::ToolCall { .. }))
        } else {
            false
        }
    });
    assert!(!has_tool_call);
}
