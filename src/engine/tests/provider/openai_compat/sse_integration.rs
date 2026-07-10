//! Integration tests: OpenAI-compat provider → wiremock SSE server → Message.

use evotengine::provider::traits::StreamConfig;
use evotengine::provider::OpenAiCompatProvider;
use evotengine::provider::StreamEvent;
use evotengine::types::*;

use super::super::fixtures::mock_server::*;
use super::super::fixtures::sse::openai as openai_sse;
use super::super::fixtures::stream_config::*;

/// OpenAI config pointing at a mock server base_url.
fn openai_config() -> StreamConfig {
    StreamConfigBuilder::openai()
        .system_prompt("You are helpful.")
        .cache_disabled()
        .build()
}

// ---------------------------------------------------------------------------
// SSE streaming — text response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_text_response() {
    let sse = openai_sse::body(vec![
        openai_sse::text_chunk("Hello, ", None),
        openai_sse::text_chunk("world!", None),
        openai_sse::finish_with_usage("stop", 50, 10),
        openai_sse::done(),
    ]);

    let (msg, events) = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
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
            assert_eq!(usage.input, 50);
            assert_eq!(usage.output, 10);
        }
        _ => panic!("Expected Assistant message"),
    }

    let text_deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_deltas, vec!["Hello, ", "world!"]);
}

#[tokio::test]
async fn openai_sse_cache_tokens_are_not_double_counted_as_input() {
    let sse = openai_sse::body(vec![
        openai_sse::text_chunk("cached", None),
        openai_sse::finish_with_cache_usage("stop", 100_000, 2_000, 80_000, 1_000),
        openai_sse::done(),
    ]);

    let (msg, _) = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
        .await
        .unwrap();

    match msg {
        Message::Assistant { usage, .. } => {
            assert_eq!(usage.input, 19_000);
            assert_eq!(usage.cache_read, 80_000);
            assert_eq!(usage.cache_write, 1_000);
            assert_eq!(usage.output, 2_000);
            assert_eq!(usage.total_tokens, 102_000);
        }
        _ => panic!("Expected Assistant message"),
    }
}

// ---------------------------------------------------------------------------
// SSE streaming — tool call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_tool_call() {
    let sse = openai_sse::body(vec![
        openai_sse::tool_call_start(0, "call_abc", "bash"),
        openai_sse::tool_call_args(0, r#"{"command": "ls"}"#),
        openai_sse::finish_with_usage("tool_calls", 40, 8),
        openai_sse::done(),
    ]);

    let (msg, events) = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
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
                    if id == "call_abc" && name == "bash" && arguments["command"] == "ls")
            );
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ToolCallStart { name, .. } if name == "bash")));
    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ToolCallEnd { .. })));
}

#[tokio::test]
async fn openai_sse_interleaved_parallel_tool_calls_keep_separate_state() {
    let sse = openai_sse::body(vec![
        openai_sse::tool_call_chunk(0, Some("call-read"), Some("read"), None),
        openai_sse::tool_call_chunk(1, Some("call-edit"), Some("edit"), None),
        openai_sse::tool_call_chunk(0, None, None, Some(r#"{"path":"src/a"}"#)),
        openai_sse::tool_call_chunk(1, None, None, Some(r#"{"path":"src/b","edits":[]}"#)),
        openai_sse::finish_with_usage("tool_calls", 40, 8),
        openai_sse::done(),
    ]);

    let (msg, events) = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
        .await
        .unwrap();

    let calls = match msg {
        Message::Assistant { content, .. } => content,
        _ => panic!("Expected Assistant message"),
    };
    assert!(matches!(
        &calls[0],
        Content::ToolCall { id, name, arguments }
            if id == "call-read" && name == "read" && arguments["path"] == "src/a"
    ));
    assert!(matches!(
        &calls[1],
        Content::ToolCall { id, name, arguments }
            if id == "call-edit" && name == "edit" && arguments["path"] == "src/b"
    ));

    let updates: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::ToolCallDelta {
                content_index,
                id,
                name,
                arguments,
            } => Some((*content_index, id.as_str(), name.as_str(), arguments)),
            _ => None,
        })
        .collect();
    assert!(updates.iter().any(|(index, id, name, args)| {
        *index == 0 && *id == "call-read" && *name == "read" && args["path"] == "src/a"
    }));
    assert!(updates.iter().any(|(index, id, name, args)| {
        *index == 1 && *id == "call-edit" && *name == "edit" && args["path"] == "src/b"
    }));
}

// ---------------------------------------------------------------------------
// SSE streaming — empty response is error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_empty_response_is_error() {
    let sse = openai_sse::body(vec![openai_sse::done()]);

    let err = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
        .await
        .unwrap_err();

    assert!(matches!(err, evotengine::provider::ProviderError::Api(_)));
}

// ---------------------------------------------------------------------------
// SSE streaming — inline error chunk
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_inline_error() {
    let sse = openai_sse::body(vec![
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [],
                "error": {"message": "upstream failed"}
            })
        ),
        openai_sse::done(),
    ]);

    let err = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        evotengine::provider::ProviderError::Api(ref msg) if msg.contains("upstream failed")
    ));
}

// ---------------------------------------------------------------------------
// SSE streaming — inline overflow error chunk
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_inline_overflow_error() {
    let sse = openai_sse::body(vec![
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [],
                "error": {
                    "message": "Your input exceeds the context window of this model. \
                                 Please adjust your input and try again."
                }
            })
        ),
        openai_sse::done(),
    ]);

    let err = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
        .await
        .unwrap_err();

    // Inline overflow errors must classify as ContextOverflow so the agent loop
    // triggers compaction instead of retrying transiently ("try again").
    assert!(
        matches!(
            err,
            evotengine::provider::ProviderError::ContextOverflow { .. }
        ),
        "expected ContextOverflow, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// HTTP error — 429 rate limit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_http_429_rate_limited() {
    let err = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        r#"{"error":{"message":"Rate limited","type":"rate_limit_error"}}"#,
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
async fn openai_http_400_context_overflow() {
    let err = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        r#"{"error":{"message":"Your input exceeds the context window of this model","type":"invalid_request_error"}}"#,
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
async fn openai_json_fallback_success() {
    let json = serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello from JSON!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 30,
            "completion_tokens": 5,
            "total_tokens": 35
        }
    });

    let (msg, events) = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        &json.to_string(),
        200,
    )
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
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello from JSON!"));
            assert_eq!(*stop_reason, StopReason::Stop);
            assert_eq!(usage.input, 30);
            assert_eq!(usage.output, 5);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events.iter().any(|e| matches!(e, StreamEvent::Start)));
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Done { .. })));
}

#[tokio::test]
async fn openai_json_fallback_cache_tokens_are_not_double_counted_as_input() {
    let json = serde_json::json!({
        "id": "chatcmpl-cache",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "cached" },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 100_000,
            "completion_tokens": 2_000,
            "total_tokens": 102_000,
            "prompt_tokens_details": {
                "cached_tokens": 80_000,
                "cache_write_tokens": 1_000
            }
        }
    });

    let (msg, _) = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        &json.to_string(),
        200,
    )
    .await
    .unwrap();

    match msg {
        Message::Assistant { usage, .. } => {
            assert_eq!(usage.input, 19_000);
            assert_eq!(usage.cache_read, 80_000);
            assert_eq!(usage.cache_write, 1_000);
            assert_eq!(usage.output, 2_000);
            assert_eq!(usage.total_tokens, 102_000);
        }
        _ => panic!("Expected Assistant message"),
    }
}

// ---------------------------------------------------------------------------
// JSON fallback — error response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_json_fallback_error() {
    let json = serde_json::json!({
        "error": {
            "message": "Internal server error",
            "type": "server_error"
        }
    });

    let err = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        &json.to_string(),
        200,
    )
    .await
    .unwrap_err();

    assert!(matches!(err, evotengine::provider::ProviderError::Api(_)));
}
