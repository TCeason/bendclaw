//! SSE event builders for OpenAI streaming format.

/// A text content delta chunk.
pub fn text_chunk(text: &str, finish_reason: Option<&str>) -> String {
    format!(
        "data: {}",
        serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {"content": text},
                "finish_reason": finish_reason
            }]
        })
    )
}

/// A tool call start chunk.
pub fn tool_call_start(index: u32, id: &str, name: &str) -> String {
    format!(
        "data: {}",
        serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": index,
                        "id": id,
                        "function": {"name": name}
                    }]
                },
                "finish_reason": null
            }]
        })
    )
}

/// A tool call argument delta chunk.
pub fn tool_call_args(index: u32, args: &str) -> String {
    format!(
        "data: {}",
        serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": index,
                        "function": {"arguments": args}
                    }]
                },
                "finish_reason": null
            }]
        })
    )
}

/// A finish chunk with usage.
pub fn finish_with_usage(
    finish_reason: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> String {
    format!(
        "data: {}",
        serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": finish_reason
            }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens
            }
        })
    )
}

/// [DONE] marker.
pub fn done() -> String {
    "data: [DONE]".into()
}

/// Join events into an SSE body.
pub fn body(events: Vec<String>) -> String {
    events.join("\n\n") + "\n\n"
}
