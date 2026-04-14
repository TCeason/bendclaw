//! Tests for OpenAI-compatible JSON fallback handling.

use bendengine::provider::stream_fallback::FallbackEmitter;
use bendengine::provider::stream_http::classify_json_error;
use bendengine::provider::ProviderError;
use bendengine::provider::StreamEvent;
use bendengine::types::*;

use super::super::fixtures::stream_config::collect_stream_events;

// ---------------------------------------------------------------------------
// Error-shaped JSON classification
// ---------------------------------------------------------------------------

#[test]
fn openai_error_generic() {
    let value = serde_json::json!({
        "error": {
            "message": "server error",
            "type": "server_error"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(bendengine::retry::should_retry(&err));
}

#[test]
fn openai_error_context_overflow() {
    let value = serde_json::json!({
        "error": {
            "message": "Your input exceeds the context window of this model",
            "type": "invalid_request_error"
        }
    });
    let err = classify_json_error(&value);
    assert!(err.is_context_overflow());
    assert!(!bendengine::retry::should_retry(&err));
}

// ---------------------------------------------------------------------------
// Success-shaped JSON → FallbackEmitter (integration-style)
// ---------------------------------------------------------------------------

#[test]
fn openai_success_text_response() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut emitter = FallbackEmitter::new(tx);

    emitter.emit_text("Hello from OpenAI!");
    emitter.set_stop_reason(StopReason::Stop);
    emitter.set_usage(Usage {
        input: 50,
        output: 10,
        total_tokens: 60,
        ..Default::default()
    });

    let msg = emitter.finalize("gpt-4o", "openai");

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            model,
            provider,
            usage,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello from OpenAI!"));
            assert_eq!(*stop_reason, StopReason::Stop);
            assert_eq!(model, "gpt-4o");
            assert_eq!(provider, "openai");
            assert_eq!(usage.input, 50);
            assert_eq!(usage.output, 10);
            assert_eq!(usage.total_tokens, 60);
        }
        _ => panic!("Expected Assistant message"),
    }

    let events = collect_stream_events(&mut rx);
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(
        matches!(&events[1], StreamEvent::TextDelta { delta, .. } if delta == "Hello from OpenAI!")
    );
    assert!(matches!(&events[2], StreamEvent::Done { .. }));
}

#[test]
fn openai_success_tool_calls_response() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut emitter = FallbackEmitter::new(tx);

    emitter.emit_tool_call(
        "call_abc123",
        "bash",
        serde_json::json!({"command": "ls -la"}),
    );
    emitter.set_stop_reason(StopReason::ToolUse);

    let msg = emitter.finalize("gpt-4o", "openai");

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(
                matches!(&content[0], Content::ToolCall { id, name, arguments } if id == "call_abc123" && name == "bash" && arguments["command"] == "ls -la")
            );
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected Assistant message"),
    }

    let events = collect_stream_events(&mut rx);
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(
        matches!(&events[1], StreamEvent::ToolCallStart { id, name, .. } if id == "call_abc123" && name == "bash")
    );
    assert!(matches!(&events[2], StreamEvent::ToolCallEnd { .. }));
    assert!(matches!(&events[3], StreamEvent::Done { .. }));
}

#[test]
fn openai_success_with_reasoning() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut emitter = FallbackEmitter::new(tx);

    emitter.emit_thinking("Let me think about this...", None);
    emitter.emit_text("The answer is 42.");

    let msg = emitter.finalize("deepseek-r1", "deepseek");

    match &msg {
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 2);
            assert!(
                matches!(&content[0], Content::Thinking { thinking, .. } if thinking == "Let me think about this...")
            );
            assert!(matches!(&content[1], Content::Text { text } if text == "The answer is 42."));
        }
        _ => panic!("Expected Assistant message"),
    }

    let events = collect_stream_events(&mut rx);
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(matches!(&events[1], StreamEvent::ThinkingDelta { .. }));
    assert!(matches!(&events[2], StreamEvent::TextDelta { .. }));
    assert!(matches!(&events[3], StreamEvent::Done { .. }));
}
