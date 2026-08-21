use evotengine::types::Content;
use evotengine::types::Message;
use evotengine::types::StopReason;
use evotengine::types::ThinkingMetadata;
use evotengine::types::Usage;
use evotengine::ThinkingOnlyGuard;

fn assistant(content: Vec<Content>) -> Message {
    Message::Assistant {
        content,
        usage: Usage::default(),
        stop_reason: StopReason::Stop,
        model: "test-model".into(),
        provider: "test-provider".into(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    }
}

fn thinking_block() -> Content {
    Content::Thinking {
        thinking: "Let me analyze this deeply...".into(),
        metadata: Some(ThinkingMetadata::Anthropic {
            signature: "sig".into(),
        }),
    }
}

fn text_block(s: &str) -> Content {
    Content::Text {
        text: s.to_string(),
    }
}

#[test]
fn thinking_only_triggers_nudge() {
    let mut guard = ThinkingOnlyGuard::new();
    let msg = assistant(vec![thinking_block()]);

    let nudge = guard.check(&msg, false);
    assert!(nudge.is_some(), "should nudge on thinking-only response");
}

#[test]
fn thinking_with_text_does_not_trigger() {
    let mut guard = ThinkingOnlyGuard::new();
    let msg = assistant(vec![thinking_block(), text_block("Here is my answer.")]);

    let nudge = guard.check(&msg, false);
    assert!(nudge.is_none(), "should not nudge when text is present");
}

#[test]
fn thinking_with_tool_calls_does_not_trigger() {
    let mut guard = ThinkingOnlyGuard::new();
    let msg = assistant(vec![thinking_block()]);

    let nudge = guard.check(&msg, true);
    assert!(
        nudge.is_none(),
        "should not nudge when tool calls are present"
    );
}

#[test]
fn text_only_does_not_trigger() {
    let mut guard = ThinkingOnlyGuard::new();
    let msg = assistant(vec![text_block("Done.")]);

    let nudge = guard.check(&msg, false);
    assert!(nudge.is_none(), "should not nudge on normal text response");
}

#[test]
fn whitespace_only_text_still_triggers() {
    let mut guard = ThinkingOnlyGuard::new();
    let msg = assistant(vec![thinking_block(), text_block("   \n  ")]);

    let nudge = guard.check(&msg, false);
    assert!(
        nudge.is_some(),
        "whitespace-only text should count as thinking-only"
    );
}

#[test]
fn max_retries_respected() {
    let mut guard = ThinkingOnlyGuard::new();
    let msg = assistant(vec![thinking_block()]);

    // First two should nudge
    assert!(guard.check(&msg, false).is_some());
    assert!(guard.check(&msg, false).is_some());
    // Third should give up
    assert!(
        guard.check(&msg, false).is_none(),
        "should stop after max retries"
    );
}

#[test]
fn resets_after_successful_response() {
    let mut guard = ThinkingOnlyGuard::new();
    let thinking_only = assistant(vec![thinking_block()]);
    let with_text = assistant(vec![thinking_block(), text_block("Here's the plan.")]);

    // Use one retry
    assert!(guard.check(&thinking_only, false).is_some());
    // Successful response resets counter
    assert!(guard.check(&with_text, false).is_none());
    // Should have full retries again
    assert!(guard.check(&thinking_only, false).is_some());
    assert!(guard.check(&thinking_only, false).is_some());
    assert!(guard.check(&thinking_only, false).is_none());
}

#[test]
fn nudge_contains_system_reminder() {
    let mut guard = ThinkingOnlyGuard::new();
    let msg = assistant(vec![thinking_block()]);

    let nudge = guard.check(&msg, false).expect("should produce nudge");
    // AgentMessage::Llm wraps a Message::User
    let inner = match nudge {
        evotengine::types::AgentMessage::Llm(m) => m,
        _ => panic!("expected AgentMessage::Llm"),
    };
    if let Message::User { content, .. } = inner {
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
        assert!(text.contains("<system-reminder>"));
        assert!(text.contains("no visible output"));
    } else {
        panic!("expected user message");
    }
}
