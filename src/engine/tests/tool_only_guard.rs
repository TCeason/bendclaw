use evotengine::types::AgentMessage;
use evotengine::types::Content;
use evotengine::types::Message;
use evotengine::types::StopReason;
use evotengine::types::Usage;
use evotengine::ToolOnlyGuard;
use serde_json::json;

fn assistant(content: Vec<Content>) -> Message {
    Message::Assistant {
        content,
        usage: Usage::default(),
        stop_reason: StopReason::ToolUse,
        model: "test-model".into(),
        provider: "test-provider".into(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    }
}

fn tool_call() -> Content {
    Content::ToolCall {
        id: "toolu_1".into(),
        name: "read".into(),
        arguments: json!({"path": "/tmp/a.rs"}),
    }
}

#[test]
fn no_trigger_below_threshold() {
    let mut guard = ToolOnlyGuard::new(3);
    let msg = assistant(vec![tool_call()]);

    assert!(guard.check(&msg, true).is_none());
    assert!(guard.check(&msg, true).is_none());
}

#[test]
fn triggers_once_at_threshold() {
    let mut guard = ToolOnlyGuard::new(3);
    let msg = assistant(vec![tool_call()]);

    assert!(guard.check(&msg, true).is_none());
    assert!(guard.check(&msg, true).is_none());
    let intervention = guard.check(&msg, true);
    assert!(intervention.is_some());
    assert!(guard.check(&msg, true).is_none());
}

#[test]
fn visible_text_resets_streak() {
    let mut guard = ToolOnlyGuard::new(3);
    let tool_only = assistant(vec![tool_call()]);
    let with_text = assistant(vec![
        Content::Text {
            text: "I found the cause.".into(),
        },
        tool_call(),
    ]);

    assert!(guard.check(&tool_only, true).is_none());
    assert!(guard.check(&tool_only, true).is_none());
    assert!(guard.check(&with_text, true).is_none());
    assert!(guard.check(&tool_only, true).is_none());
}

#[test]
fn no_tool_call_resets_streak() {
    let mut guard = ToolOnlyGuard::new(2);
    let tool_only = assistant(vec![tool_call()]);
    let final_text = assistant(vec![Content::Text {
        text: "Done.".into(),
    }]);

    assert!(guard.check(&tool_only, true).is_none());
    assert!(guard.check(&final_text, false).is_none());
    assert!(guard.check(&tool_only, true).is_none());
}

#[test]
fn intervention_is_user_steering_message() {
    let mut guard = ToolOnlyGuard::new(1);
    let msg = assistant(vec![tool_call()]);

    let intervention = guard.check(&msg, true);
    let Some(intervention) = intervention else {
        panic!("expected intervention");
    };

    assert_eq!(intervention.steering_message.role(), "user");
    if let AgentMessage::Llm(Message::User { content, .. }) = intervention.steering_message {
        let text = content
            .iter()
            .filter_map(|c| {
                if let Content::Text { text } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<String>();
        assert!(text.contains("Status:"));
        assert!(text.contains("short progress update"));
    } else {
        panic!("expected user steering message");
    }
}
