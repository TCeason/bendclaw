use evotengine::context::compaction::marker::build_full_marker;
use evotengine::types::*;

fn make_assistant_with_tool_call(id: &str, name: &str, args: serde_json::Value) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: args,
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

fn make_tool_result(id: &str, name: &str, text: &str, is_error: bool) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: id.to_string(),
        tool_name: name.to_string(),
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        is_error,
        timestamp: 0,
        retention: Retention::Normal,
    })
}

fn make_assistant_text(text: &str) -> AgentMessage {
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

fn marker_text(messages: &[AgentMessage], removed: usize) -> String {
    let marker = build_full_marker(messages, removed);
    match marker {
        AgentMessage::Llm(Message::User { content, .. }) => content
            .into_iter()
            .find_map(|c| match c {
                Content::Text { text } => Some(text),
                _ => None,
            })
            .unwrap_or_default(),
        _ => panic!("expected user message"),
    }
}

#[test]
fn marker_includes_file_modifications() {
    let messages = vec![
        make_assistant_with_tool_call(
            "c1",
            "Edit",
            serde_json::json!({"file_path": "/src/main.rs", "old_string": "a", "new_string": "b"}),
        ),
        make_tool_result("c1", "Edit", "File updated successfully.", false),
        make_assistant_with_tool_call(
            "c2",
            "Write",
            serde_json::json!({"file_path": "/tests/new.rs", "content": "fn test() {}"}),
        ),
        make_tool_result("c2", "Write", "File created successfully.", false),
        // Error edit should be excluded
        make_assistant_with_tool_call(
            "c3",
            "Edit",
            serde_json::json!({"file_path": "/src/bad.rs", "old_string": "x", "new_string": "y"}),
        ),
        make_tool_result("c3", "Edit", "old_string not found", true),
    ];

    let text = marker_text(&messages, 6);
    assert!(text.contains("/src/main.rs (edited)"));
    assert!(text.contains("/tests/new.rs (created)"));
    assert!(!text.contains("/src/bad.rs"));
}

#[test]
fn marker_deduplicates_file_modifications() {
    let messages = vec![
        make_assistant_with_tool_call(
            "c1",
            "Edit",
            serde_json::json!({"file_path": "/src/main.rs", "old_string": "a", "new_string": "b"}),
        ),
        make_tool_result("c1", "Edit", "ok", false),
        make_assistant_with_tool_call(
            "c2",
            "Edit",
            serde_json::json!({"file_path": "/src/main.rs", "old_string": "b", "new_string": "c"}),
        ),
        make_tool_result("c2", "Edit", "ok", false),
    ];

    let text = marker_text(&messages, 4);
    assert_eq!(text.matches("/src/main.rs").count(), 1);
}

#[test]
fn marker_includes_env_discoveries() {
    let messages = vec![
        make_assistant_with_tool_call("c1", "Bash", serde_json::json!({"command": "which cargo"})),
        make_tool_result(
            "c1",
            "Bash",
            "/root/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/cargo",
            false,
        ),
    ];

    let text = marker_text(&messages, 2);
    assert!(text.contains("/bin/cargo"));
}

#[test]
fn marker_skips_non_probe_bash_commands() {
    let messages = vec![
        make_assistant_with_tool_call("c1", "Bash", serde_json::json!({"command": "cargo build"})),
        make_tool_result("c1", "Bash", "error: linker not found", false),
    ];

    let text = marker_text(&messages, 2);
    assert!(!text.contains("Environment"));
}

#[test]
fn marker_includes_last_assistant_conclusion() {
    let messages = vec![
        make_assistant_text("The fix is to use MapKey wrapper instead of raw deserializer."),
        make_assistant_text("done."),
    ];

    let text = marker_text(&messages, 2);
    assert!(text.contains("MapKey wrapper"));
}

#[test]
fn marker_skips_summary_and_filler_text() {
    let messages = vec![
        make_assistant_text("The real conclusion here."),
        make_assistant_text("[Summary] Edit, Bash"),
        make_assistant_text("done."),
    ];

    let text = marker_text(&messages, 3);
    assert!(text.contains("The real conclusion here."));
    assert!(!text.contains("[Summary]"));
}

#[test]
fn marker_includes_all_sections() {
    let messages = vec![
        AgentMessage::Llm(Message::User {
            content: vec![Content::Text {
                text: "Fix the bug in src/de.rs".into(),
            }],
            timestamp: 0,
        }),
        make_assistant_with_tool_call(
            "c1",
            "Edit",
            serde_json::json!({"file_path": "/src/de.rs", "old_string": "a", "new_string": "b"}),
        ),
        make_tool_result("c1", "Edit", "ok", false),
        make_assistant_with_tool_call("c2", "Bash", serde_json::json!({"command": "which cargo"})),
        make_tool_result("c2", "Bash", "/usr/bin/cargo", false),
        make_assistant_text("The fix uses MapKey to enforce string-only keys."),
    ];

    let text = marker_text(&messages, 6);
    assert!(text.contains("6 messages removed"));
    assert!(text.contains("/src/de.rs (edited)"));
    assert!(text.contains("/usr/bin/cargo"));
    assert!(text.contains("MapKey to enforce string-only keys"));
    assert!(text.contains("Fix the bug in src/de.rs"));
}
