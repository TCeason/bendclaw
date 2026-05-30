//! Tests for the compact memory extraction module.

use evotengine::context::compaction::memory::MemoryInput;
use evotengine::context::compaction::memory::{self};
use evotengine::context::compaction::types::CompactionState;
use evotengine::types::*;

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
fn tool_result_msg(id: &str, name: &str, content: &str) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: id.to_string(),
        tool_name: name.to_string(),
        content: vec![Content::Text {
            text: content.to_string(),
        }],
        is_error: false,
        timestamp: 0,
        retention: Retention::Normal,
    })
}

fn extract_memory_text(msg: &AgentMessage) -> &str {
    match msg {
        AgentMessage::Llm(Message::User { content, .. }) => {
            if let Some(Content::Text { text }) = content.first() {
                text.as_str()
            } else {
                ""
            }
        }
        _ => "",
    }
}

#[test]
fn memory_summary_contains_message_count() {
    let evicted = vec![user_msg("hello"), assistant_msg("hi")];
    let input = MemoryInput {
        evicted: &evicted,
        split_turn_prefix: None,
        prev_state: None,
    };
    let memory_summary = memory::build(&input);
    let text = extract_memory_text(&memory_summary);
    assert!(text.contains("2 messages removed"));
}

#[test]
fn memory_extracts_user_requests() {
    let evicted = vec![
        user_msg("Fix the typo in main.rs"),
        assistant_msg("Done, fixed the typo."),
        user_msg("Add a test for the parser"),
        assistant_msg("Added parser_test.rs"),
    ];
    let input = MemoryInput {
        evicted: &evicted,
        split_turn_prefix: None,
        prev_state: None,
    };
    let memory_summary = memory::build(&input);
    let text = extract_memory_text(&memory_summary);
    assert!(text.contains("Completed requests"));
    assert!(text.contains("Fix the typo"));
    assert!(text.contains("Add a test"));
}

#[test]
fn memory_extracts_file_ops() {
    let evicted = vec![
        user_msg("read the config"),
        tool_call_msg("c1", "Read", "/src/config.rs"),
        tool_result_msg("c1", "Read", "pub struct Config {}"),
        tool_call_msg("c2", "Write", "/src/new_file.rs"),
        tool_result_msg("c2", "Write", "ok"),
        assistant_msg("Done."),
    ];
    let input = MemoryInput {
        evicted: &evicted,
        split_turn_prefix: None,
        prev_state: None,
    };
    let memory_summary = memory::build(&input);
    let text = extract_memory_text(&memory_summary);
    assert!(text.contains("config.rs") || text.contains("Files read"));
    assert!(text.contains("new_file.rs") || text.contains("Files modified"));
}

#[test]
fn memory_includes_last_conclusion() {
    let evicted = vec![
        user_msg("explain the architecture"),
        assistant_msg("The system uses a layered approach with clear separation of concerns."),
    ];
    let input = MemoryInput {
        evicted: &evicted,
        split_turn_prefix: None,
        prev_state: None,
    };
    let memory_summary = memory::build(&input);
    let text = extract_memory_text(&memory_summary);
    assert!(text.contains("Last assistant conclusion"));
    assert!(text.contains("layered approach"));
}

#[test]
fn memory_with_split_turn_prefix() {
    let prefix = vec![
        user_msg("refactor the module"),
        tool_call_msg("c1", "Edit", "/src/mod.rs"),
        tool_result_msg("c1", "Edit", "updated"),
    ];
    let evicted = vec![
        user_msg("first task"),
        assistant_msg("done"),
        user_msg("refactor the module"),
        tool_call_msg("c1", "Edit", "/src/mod.rs"),
        tool_result_msg("c1", "Edit", "updated"),
    ];
    let input = MemoryInput {
        evicted: &evicted,
        split_turn_prefix: Some(&prefix),
        prev_state: None,
    };
    let memory_summary = memory::build(&input);
    let text = extract_memory_text(&memory_summary);
    assert!(text.contains("Current turn context"));
    assert!(text.contains("refactor the module"));
}

#[test]
fn memory_accumulates_state_from_prev() {
    let mut prev_file_ops = evotengine::context::compaction::types::FileOps::default();
    prev_file_ops.read.insert("/old/file.rs".to_string());
    let prev_state = CompactionState {
        file_ops: prev_file_ops,
        env_discoveries: vec!["rust 1.75".to_string()],
        completed_requests: vec!["old request".to_string()],
        timestamp: 1000,
        generation: 1,
        last_summary: None,
    };

    let evicted = vec![
        user_msg("new task"),
        tool_call_msg("c1", "Read", "/new/file.rs"),
        tool_result_msg("c1", "Read", "content"),
        assistant_msg("read it"),
    ];

    let state = memory::build_state(&evicted, None, Some(&prev_state));
    assert!(state.file_ops.read.contains("/old/file.rs"));
    assert!(state.file_ops.read.contains("/new/file.rs"));
    assert_eq!(state.generation, 2);
    assert!(state
        .completed_requests
        .contains(&"old request".to_string()));
}

#[test]
fn memory_summary_is_user_message() {
    let evicted = vec![user_msg("hi"), assistant_msg("hello")];
    let input = MemoryInput {
        evicted: &evicted,
        split_turn_prefix: None,
        prev_state: None,
    };
    let memory_summary = memory::build(&input);
    assert!(matches!(
        memory_summary,
        AgentMessage::Llm(Message::User { .. })
    ));
}
