//! SSE event builders for Anthropic streaming format.

/// message_start event with usage.
pub fn message_start(input_tokens: u64, cache_read: u64) -> String {
    format!(
        "event: message_start\ndata: {}",
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-20250514",
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": 0,
                    "cache_read_input_tokens": cache_read,
                    "cache_creation_input_tokens": 0
                }
            }
        })
    )
}

/// content_block_start for text.
pub fn text_block_start(index: u64) -> String {
    format!(
        "event: content_block_start\ndata: {}",
        serde_json::json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {"type": "text", "text": ""}
        })
    )
}

/// content_block_delta for text.
pub fn text_delta(index: u64, text: &str) -> String {
    format!(
        "event: content_block_delta\ndata: {}",
        serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "text_delta", "text": text}
        })
    )
}

/// content_block_stop.
pub fn block_stop(index: u64) -> String {
    format!(
        "event: content_block_stop\ndata: {}",
        serde_json::json!({"type": "content_block_stop", "index": index})
    )
}

/// content_block_start for tool_use.
pub fn tool_block_start(index: u64, id: &str, name: &str) -> String {
    format!(
        "event: content_block_start\ndata: {}",
        serde_json::json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {"type": "tool_use", "id": id, "name": name}
        })
    )
}

/// content_block_delta for tool input JSON.
pub fn tool_input_delta(index: u64, partial_json: &str) -> String {
    format!(
        "event: content_block_delta\ndata: {}",
        serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "input_json_delta", "partial_json": partial_json}
        })
    )
}

/// content_block_start for thinking.
pub fn thinking_block_start(index: u64) -> String {
    format!(
        "event: content_block_start\ndata: {}",
        serde_json::json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {"type": "thinking", "thinking": ""}
        })
    )
}

/// content_block_delta for thinking.
pub fn thinking_delta(index: u64, text: &str) -> String {
    format!(
        "event: content_block_delta\ndata: {}",
        serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "thinking_delta", "thinking": text}
        })
    )
}

/// message_delta with stop_reason and output usage.
pub fn message_delta(stop_reason: &str, output_tokens: u64) -> String {
    format!(
        "event: message_delta\ndata: {}",
        serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": stop_reason},
            "usage": {"output_tokens": output_tokens, "input_tokens": 0}
        })
    )
}

/// message_stop event.
pub fn message_stop() -> String {
    "event: message_stop\ndata: {\"type\":\"message_stop\"}".into()
}

/// error event.
pub fn error(error_type: &str, message: &str) -> String {
    format!(
        "event: error\ndata: {}",
        serde_json::json!({
            "type": error_type,
            "message": message
        })
    )
}

/// Join events into an SSE body.
pub fn body(events: Vec<String>) -> String {
    events.join("\n\n") + "\n\n"
}
