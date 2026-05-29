//! Tests for the summarizer subsystem.

use evotengine::context::compaction::summarizer::mode::SummarizerMode;
use evotengine::context::compaction::summarizer::rule_based;
use evotengine::context::compaction::summarizer::serialize;
use evotengine::context::compaction::summarizer::types::SummarizerInput;
use evotengine::context::compaction::types::FileOps;
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
    let mut args = serde_json::Map::new();
    args.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: serde_json::Value::Object(args),
        }],
        stop_reason: StopReason::ToolUse,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn tool_result_msg(id: &str, content: &str) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: id.to_string(),
        tool_name: "read".to_string(),
        content: vec![Content::Text {
            text: content.to_string(),
        }],
        is_error: false,
        timestamp: 0,
        retention: Retention::Normal,
    })
}

// ---------------------------------------------------------------------------
// serialize tests
// ---------------------------------------------------------------------------

#[test]
fn serialize_messages_formats_user_and_assistant() {
    let messages = vec![user_msg("Fix the bug"), assistant_msg("I'll look into it")];
    let text = serialize::serialize_messages(&messages);
    assert!(text.contains("[User]: Fix the bug"));
    assert!(text.contains("[Assistant]: I'll look into it"));
}

#[test]
fn serialize_messages_formats_tool_calls() {
    let messages = vec![tool_call_msg("c1", "read", "/src/main.rs")];
    let text = serialize::serialize_messages(&messages);
    assert!(text.contains("[Assistant tool calls]:"));
    assert!(text.contains("read("));
    assert!(text.contains("/src/main.rs"));
}

#[test]
fn serialize_messages_truncates_long_tool_results() {
    let long_content = "x".repeat(5000);
    let messages = vec![tool_result_msg("c1", &long_content)];
    let text = serialize::serialize_messages(&messages);
    assert!(text.contains("[Tool result]:"));
    assert!(text.contains("more characters truncated"));
    // Should be much shorter than the original
    assert!(text.len() < 3000);
}

#[test]
fn serialize_messages_includes_thinking() {
    let msg = AgentMessage::Llm(Message::Assistant {
        content: vec![
            Content::Thinking {
                thinking: "Let me analyze this".to_string(),
                signature: None,
            },
            Content::Text {
                text: "Here's my answer".to_string(),
            },
        ],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    });
    let text = serialize::serialize_messages(&[msg]);
    assert!(text.contains("[Assistant thinking]: Let me analyze this"));
    assert!(text.contains("[Assistant]: Here's my answer"));
}

#[test]
fn prepare_input_extracts_all_fields() {
    let messages = vec![
        user_msg("Fix the config"),
        tool_call_msg("c1", "edit", "/src/config.rs"),
        tool_result_msg("c1", "ok"),
        assistant_msg("Config has been fixed with the new approach"),
    ];
    let input = serialize::prepare_input(&messages, None, None);

    assert_eq!(input.evicted_count, 4);
    assert!(!input.completed_requests.is_empty());
    assert!(input.completed_requests[0].contains("Fix the config"));
    assert!(input.file_ops.edited.contains("/src/config.rs"));
    assert!(input.last_conclusion.is_some());
    assert!(input.conversation.contains("[User]: Fix the config"));
    assert!(input.turn_prefix.is_none());
    assert!(input.previous_summary.is_none());
}

#[test]
fn prepare_input_with_split_prefix() {
    let messages = vec![user_msg("task 1"), assistant_msg("done")];
    let prefix = vec![user_msg("big task"), tool_call_msg("c1", "read", "/a.rs")];
    let input = serialize::prepare_input(&messages, Some(&prefix), None);

    match input.turn_prefix.as_ref() {
        Some(prefix) => assert!(prefix.contains("big task")),
        None => panic!("expected split turn prefix"),
    }
}

// ---------------------------------------------------------------------------
// rule_based tests
// ---------------------------------------------------------------------------

#[test]
fn rule_based_includes_message_count() {
    let input = SummarizerInput {
        conversation: String::new(),
        turn_prefix: None,
        previous_summary: None,
        file_ops: FileOps::default(),
        evicted_count: 15,
        completed_requests: vec![],
        env_discoveries: vec![],
        last_conclusion: None,
    };
    let output = rule_based::summarize(&input);
    assert!(output.summary.contains("15 messages removed"));
}

#[test]
fn rule_based_includes_completed_requests() {
    let input = SummarizerInput {
        conversation: String::new(),
        turn_prefix: None,
        previous_summary: None,
        file_ops: FileOps::default(),
        evicted_count: 5,
        completed_requests: vec!["Fix bug #123".into(), "Add tests".into()],
        env_discoveries: vec![],
        last_conclusion: None,
    };
    let output = rule_based::summarize(&input);
    assert!(output.summary.contains("Completed requests"));
    assert!(output.summary.contains("Fix bug #123"));
    assert!(output.summary.contains("Add tests"));
}

#[test]
fn rule_based_includes_file_ops() {
    let mut file_ops = FileOps::default();
    file_ops.edited.insert("/src/main.rs".into());
    file_ops.read.insert("/src/lib.rs".into());

    let input = SummarizerInput {
        conversation: String::new(),
        turn_prefix: None,
        previous_summary: None,
        file_ops,
        evicted_count: 5,
        completed_requests: vec![],
        env_discoveries: vec![],
        last_conclusion: None,
    };
    let output = rule_based::summarize(&input);
    assert!(output.summary.contains("Files modified"));
    assert!(output.summary.contains("/src/main.rs"));
    assert!(output.summary.contains("Files read"));
    assert!(output.summary.contains("/src/lib.rs"));
}

#[test]
fn rule_based_includes_conclusion() {
    let input = SummarizerInput {
        conversation: String::new(),
        turn_prefix: None,
        previous_summary: None,
        file_ops: FileOps::default(),
        evicted_count: 3,
        completed_requests: vec![],
        env_discoveries: vec![],
        last_conclusion: Some("All tests pass now".into()),
    };
    let output = rule_based::summarize(&input);
    assert!(output.summary.contains("Last assistant conclusion"));
    assert!(output.summary.contains("All tests pass now"));
}

#[test]
fn rule_based_includes_turn_prefix() {
    let input = SummarizerInput {
        conversation: String::new(),
        turn_prefix: Some(
            "User asked: refactor auth module\nTools used: read(/src/auth.rs)".into(),
        ),
        previous_summary: None,
        file_ops: FileOps::default(),
        evicted_count: 10,
        completed_requests: vec![],
        env_discoveries: vec![],
        last_conclusion: None,
    };
    let output = rule_based::summarize(&input);
    assert!(output.summary.contains("Current turn context"));
    assert!(output.summary.contains("refactor auth module"));
}

// ---------------------------------------------------------------------------
// mode dispatch tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mode_rule_based_returns_ok() {
    let input = SummarizerInput {
        conversation: "[User]: hello".into(),
        turn_prefix: None,
        previous_summary: None,
        file_ops: FileOps::default(),
        evicted_count: 2,
        completed_requests: vec!["hello".into()],
        env_discoveries: vec![],
        last_conclusion: Some("hi".into()),
    };
    let cancel = CancellationToken::new();
    let result = SummarizerMode::RuleBased
        .summarize(input, None, cancel)
        .await;
    let output = match result {
        Ok(output) => output,
        Err(err) => panic!("expected rule-based summarizer to succeed: {err:?}"),
    };
    assert!(output.summary.contains("2 messages removed"));
}

#[tokio::test]
async fn mode_llm_without_context_returns_error() {
    let input = SummarizerInput {
        conversation: "[User]: hello".into(),
        turn_prefix: None,
        previous_summary: None,
        file_ops: FileOps::default(),
        evicted_count: 2,
        completed_requests: vec![],
        env_discoveries: vec![],
        last_conclusion: None,
    };
    let cancel = CancellationToken::new();
    let mode = SummarizerMode::Llm { max_tokens: 4096 };
    let result = mode.summarize(input, None, cancel).await;
    assert!(result.is_err());
}
