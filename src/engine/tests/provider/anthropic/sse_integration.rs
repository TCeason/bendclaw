//! Integration tests: Anthropic provider → wiremock SSE server → Message.

use evotengine::provider::AnthropicProvider;
use evotengine::provider::StreamEvent;
use evotengine::types::*;

use super::super::fixtures::mock_server::*;
use super::super::fixtures::sse::anthropic as anthropic_sse;
use super::super::fixtures::stream_config::*;

// ---------------------------------------------------------------------------
// SSE streaming — text response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_text_response() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(100, 0),
        anthropic_sse::text_block_start(0),
        anthropic_sse::text_delta(0, "Hello, "),
        anthropic_sse::text_delta(0, "world!"),
        anthropic_sse::block_stop(0),
        anthropic_sse::message_delta("end_turn", 10),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, events) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            usage,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello, world!"));
            assert_eq!(*stop_reason, StopReason::Stop);
            assert_eq!(usage.input, 100);
            assert_eq!(usage.output, 10);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events.iter().any(|e| matches!(e, StreamEvent::Start)));
    let text_deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_deltas, vec!["Hello, ", "world!"]);
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Done { .. })));
}

// ---------------------------------------------------------------------------
// SSE streaming — tool call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_tool_call() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::tool_block_start(0, "toolu_123", "Bash"),
        anthropic_sse::tool_input_delta(0, r#"{"command": "ls -la"}"#),
        anthropic_sse::block_stop(0),
        anthropic_sse::message_delta("tool_use", 5),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, events) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(
                matches!(&content[0], Content::ToolCall { id, name, arguments }
                    if id == "toolu_123" && name == "Bash" && arguments["command"] == "ls -la")
            );
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ToolCallStart { name, .. } if name == "Bash")));
    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ToolCallEnd { .. })));
}

#[tokio::test]
async fn anthropic_sse_tool_use_error_becomes_recovery_tool_call() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::tool_block_start(0, "toolu_123", "Write"),
        anthropic_sse::tool_input_delta(0, r#"{"path":"/tmp/a.txt""#),
        anthropic_sse::block_stop(0),
        anthropic_sse::error("overloaded_error", "Overloaded"),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (outcome, events) = run_provider_sse_outcome(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        evotengine::provider::StreamOutcome::IncompleteToolUse { .. }
    ));
    let msg = outcome.into_message();

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            ..
        } => {
            assert_eq!(*stop_reason, StopReason::ToolUse);
            assert_eq!(content.len(), 1);
            match &content[0] {
                Content::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    assert_eq!(id, "toolu_123");
                    assert_eq!(name, "Write");
                    assert_eq!(arguments["path"], "/tmp/a.txt");
                    assert!(arguments.get("__evot_tool_use_truncated").is_none());
                }
                other => panic!("Expected tool call, got {other:?}"),
            }
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events.iter().any(|e| matches!(e, StreamEvent::Done { .. })));
}

#[tokio::test]
async fn anthropic_sse_tool_use_error_without_block_stop_finalizes_partial_json() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::tool_block_start(0, "toolu_123", "Write"),
        anthropic_sse::tool_input_delta(0, r#"{"path":"/tmp/a.txt""#),
        anthropic_sse::error("overloaded_error", "Overloaded"),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (outcome, _events) = run_provider_sse_outcome(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        evotengine::provider::StreamOutcome::IncompleteToolUse { .. }
    ));
    let msg = outcome.into_message();

    match &msg {
        Message::Assistant { content, .. } => match &content[0] {
            Content::ToolCall { arguments, .. } => {
                assert_eq!(arguments["path"], "/tmp/a.txt");
                assert!(arguments.get("__partial_json").is_none());
            }
            other => panic!("Expected tool call, got {other:?}"),
        },
        _ => panic!("Expected Assistant message"),
    }
}

#[tokio::test]
async fn anthropic_sse_error_before_tool_input_still_errors() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::tool_block_start(0, "toolu_123", "Write"),
        anthropic_sse::error("overloaded_error", "Overloaded"),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let err = run_provider_sse_outcome(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Overloaded"));
}

#[tokio::test]
async fn anthropic_sse_complete_tool_use_error_before_message_stop_recovers() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::tool_block_start(0, "toolu_123", "Bash"),
        anthropic_sse::tool_input_delta(0, r#"{"command":"ls -la"}"#),
        anthropic_sse::block_stop(0),
        anthropic_sse::message_delta("tool_use", 5),
        anthropic_sse::error("overloaded_error", "Overloaded"),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (outcome, events) = run_provider_sse_outcome(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        evotengine::provider::StreamOutcome::IncompleteToolUse { .. }
    ));
    let msg = outcome.into_message();

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            usage,
            ..
        } => {
            assert_eq!(*stop_reason, StopReason::ToolUse);
            assert_eq!(usage.input, 50);
            assert_eq!(usage.output, 5);
            assert!(
                matches!(&content[0], Content::ToolCall { id, name, arguments }
                if id == "toolu_123" && name == "Bash" && arguments["command"] == "ls -la")
            );
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ToolCallEnd { .. })));
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Done { .. })));
}
#[tokio::test]
async fn anthropic_sse_multiple_tool_use_error_keeps_all_tool_calls() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::tool_block_start(0, "toolu_1", "Read"),
        anthropic_sse::tool_input_delta(0, r#"{"path":"/tmp/a.txt"}"#),
        anthropic_sse::block_stop(0),
        anthropic_sse::tool_block_start(1, "toolu_2", "Write"),
        anthropic_sse::tool_input_delta(1, r#"{"path":"/tmp/b.txt""#),
        anthropic_sse::error("overloaded_error", "Overloaded"),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (outcome, _events) = run_provider_sse_outcome(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        evotengine::provider::StreamOutcome::IncompleteToolUse { .. }
    ));
    let msg = outcome.into_message();

    match &msg {
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 2);
            assert!(
                matches!(&content[0], Content::ToolCall { id, arguments, .. } if id == "toolu_1" && arguments["path"] == "/tmp/a.txt")
            );
            assert!(
                matches!(&content[1], Content::ToolCall { id, arguments, .. } if id == "toolu_2" && arguments["path"] == "/tmp/b.txt")
            );
        }
        _ => panic!("Expected Assistant message"),
    }
}

// ---------------------------------------------------------------------------
// SSE streaming — thinking + text
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_thinking_then_text() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(80, 0),
        anthropic_sse::thinking_block_start(0),
        anthropic_sse::thinking_delta(0, "Let me think..."),
        anthropic_sse::block_stop(0),
        anthropic_sse::text_block_start(1),
        anthropic_sse::text_delta(1, "The answer is 42."),
        anthropic_sse::block_stop(1),
        anthropic_sse::message_delta("end_turn", 20),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, events) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 2);
            assert!(
                matches!(&content[0], Content::Thinking { thinking, .. } if thinking == "Let me think...")
            );
            assert!(matches!(&content[1], Content::Text { text } if text == "The answer is 42."));
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ThinkingDelta { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::TextDelta { .. })));
}

// ---------------------------------------------------------------------------
// SSE streaming — error event (overloaded)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_error_event() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::error("overloaded_error", "Overloaded"),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let err = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap_err();

    assert!(evotengine::retry::should_retry(&err));
}

// ---------------------------------------------------------------------------
// SSE streaming — usage with cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_cache_usage() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(100, 500),
        anthropic_sse::text_block_start(0),
        anthropic_sse::text_delta(0, "cached"),
        anthropic_sse::block_stop(0),
        anthropic_sse::message_delta("end_turn", 5),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, _) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant { usage, .. } => {
            assert_eq!(usage.input, 100);
            assert_eq!(usage.cache_read, 500);
        }
        _ => panic!("Expected Assistant message"),
    }
}

// Some Anthropic-compatible proxies report cache tokens only in the final
// `message_delta`, leaving `message_start.usage` as zero. The decoder must
// pick up cache_read/cache_write from `message_delta.usage` as well.
#[tokio::test]
async fn anthropic_sse_cache_usage_in_message_delta() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(100, 0),
        anthropic_sse::text_block_start(0),
        anthropic_sse::text_delta(0, "cached"),
        anthropic_sse::block_stop(0),
        anthropic_sse::message_delta_with_usage("end_turn", 100, 5, 500, 100),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, _) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant { usage, .. } => {
            assert_eq!(usage.input, 100);
            assert_eq!(usage.output, 5);
            assert_eq!(usage.cache_read, 500);
            assert_eq!(usage.cache_write, 100);
        }
        _ => panic!("Expected Assistant message"),
    }
}

// ---------------------------------------------------------------------------
// HTTP error — 429 rate limit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_http_429_rate_limited() {
    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let err = run_provider_json(
        &AnthropicProvider,
        config,
        r#"{"error":{"type":"rate_limit_error","message":"Rate limited"}}"#,
        429,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        evotengine::provider::ProviderError::RateLimited { .. }
    ));
}

// ---------------------------------------------------------------------------
// HTTP error — 400 context overflow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_http_400_context_overflow() {
    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let err = run_provider_json(
        &AnthropicProvider,
        config,
        r#"{"error":{"type":"invalid_request_error","message":"prompt is too long: 213462 tokens > 200000 maximum"}}"#,
        400,
    )
    .await
    .unwrap_err();

    assert!(err.is_context_overflow());
}

// ---------------------------------------------------------------------------
// JSON fallback — success response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_json_fallback_success() {
    let json = serde_json::json!({
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello from JSON!"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 50, "output_tokens": 10}
    });

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, events) = run_provider_json(&AnthropicProvider, config, &json.to_string(), 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant { content, usage, .. } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello from JSON!"));
            assert_eq!(usage.input, 50);
            assert_eq!(usage.output, 10);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events.iter().any(|e| matches!(e, StreamEvent::Start)));
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Done { .. })));
}
