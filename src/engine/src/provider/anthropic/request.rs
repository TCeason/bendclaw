//! Anthropic request body building and content conversion.

use crate::provider::traits::StreamConfig;
use crate::types::*;

const DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

/// Build the Anthropic Messages API URL, avoiding a doubled `/v1` when the
/// base already ends in `/v1`.
pub fn build_messages_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

pub fn build_request_body(config: &StreamConfig, is_oauth: bool) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for msg in &config.messages {
        match msg {
            Message::User { content, .. } => {
                let blocks = content_to_anthropic(content);
                if blocks.is_empty() {
                    continue;
                }
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": blocks,
                }));
            }
            Message::Assistant { content, .. } => {
                let blocks = content_to_anthropic(content);
                // Match pi: aborted/empty assistant messages carry no semantic
                // content and must not be turned into prompt text. Also discard
                // the exact sentinel written by older evot builds so existing
                // sessions can resume without teaching it to the model.
                if blocks.is_empty() || is_legacy_empty_response(content) {
                    continue;
                }
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": blocks,
                }));
            }
            Message::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let result_content = if content.iter().any(|c| matches!(c, Content::Image { .. })) {
                    // Multi-content with images: use array format
                    serde_json::json!(content_to_anthropic(content))
                } else {
                    // Text-only: use string shorthand
                    let text = content
                        .iter()
                        .find_map(|c| match c {
                            Content::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    serde_json::json!(text)
                };

                messages.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": result_content,
                        "is_error": is_error,
                    }],
                }));
            }
        }
    }

    // Merge consecutive messages with the same role — Anthropic API requires
    // strict user/assistant alternation. Consecutive same-role messages can
    // arise when empty assistant responses are skipped above.
    let mut messages = merge_consecutive_roles(messages);

    // -----------------------------------------------------------------------
    // Prompt caching — place cache_control breakpoints based on CacheConfig.
    //
    // Anthropic caches the full prefix (tools → system → messages) up to each
    // breakpoint. We use up to 3 breakpoints:
    //   1. System prompt (stable across turns)
    //   2. Last tool definition (tools rarely change)
    //   3. Second-to-last message (conversation history grows, cache the prefix)
    //
    // When caching is disabled or strategy is Disabled, no markers are added.
    // -----------------------------------------------------------------------
    let cache = &config.cache_config;
    let caching_enabled = cache.enabled && cache.strategy != CacheStrategy::Disabled;
    let (cache_system, cache_tools, cache_messages) = match &cache.strategy {
        CacheStrategy::Auto => (true, true, true),
        CacheStrategy::Disabled => (false, false, false),
        CacheStrategy::Manual {
            cache_system,
            cache_tools,
            cache_messages,
        } => (*cache_system, *cache_tools, *cache_messages),
    };

    // Breakpoint 3: scan backwards from second-to-last message to find one with
    // non-empty content to place the cache breakpoint on
    if caching_enabled && cache_messages && messages.len() >= 2 {
        for idx in (0..messages.len() - 1).rev() {
            if let Some(content) = messages[idx]["content"].as_array_mut() {
                if let Some(last_block) = content.last_mut() {
                    let is_empty_text = last_block.get("type").and_then(|t| t.as_str())
                        == Some("text")
                        && last_block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .is_empty();
                    if !is_empty_text {
                        last_block["cache_control"] = serde_json::json!({"type": "ephemeral"});
                        break;
                    }
                }
            }
        }
    }

    let thinking_level = crate::provider::thinking::effective_thinking_level(
        config.thinking_level,
        config.model_config.as_ref(),
    );
    let adaptive = crate::provider::thinking::force_adaptive_thinking(config.model_config.as_ref());
    let model_max = config
        .model_config
        .as_ref()
        .map(|m| m.max_tokens)
        .unwrap_or_else(|| config.requested_max_tokens());

    // Budget thinking expands an explicit visible-output cap first; the final
    // request is then clamped to remaining context, matching pi's ordering.
    let (requested_max_tokens, requested_thinking_budget) =
        if thinking_level != ThinkingLevel::Off && !adaptive {
            crate::provider::thinking::adjust_max_tokens_for_thinking(
                config.max_tokens,
                model_max,
                thinking_level,
            )
        } else {
            (config.requested_max_tokens(), 0)
        };
    let max_tokens = config.clamp_max_tokens_to_context(requested_max_tokens);
    let thinking_budget = requested_thinking_budget
        .min(max_tokens.saturating_sub(crate::provider::thinking::MIN_OUTPUT_AFTER_THINKING));

    let mut body = serde_json::json!({
        "model": config.model,
        "max_tokens": max_tokens,
        "stream": true,
        "messages": messages,
    });

    // Breakpoint 1: system prompt
    if is_oauth {
        let mut system_blocks = vec![serde_json::json!({
            "type": "text",
            "text": "You are Claude Code, Anthropic's official CLI for Claude.",
        })];
        system_blocks.extend(system_prompt_blocks(
            &config.system_prompt,
            caching_enabled && cache_system,
        ));
        body["system"] = serde_json::json!(system_blocks);
    } else if !config.system_prompt.is_empty() {
        body["system"] = serde_json::json!(system_prompt_blocks(
            &config.system_prompt,
            caching_enabled && cache_system,
        ));
    }

    // Breakpoint 2: last tool definition (tools are stable between turns)
    if !config.tools.is_empty() {
        let mut tools: Vec<serde_json::Value> = config
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();
        if caching_enabled && cache_tools {
            if let Some(last_tool) = tools.last_mut() {
                last_tool["cache_control"] = serde_json::json!({"type": "ephemeral"});
            }
        }
        body["tools"] = serde_json::json!(tools);
    }

    if thinking_level != ThinkingLevel::Off {
        if adaptive {
            body["thinking"] = serde_json::json!({
                "type": "adaptive",
                "display": "summarized",
            });
            if let Some(effort) = crate::provider::thinking::anthropic_effort(
                thinking_level,
                config.model_config.as_ref(),
            ) {
                body["output_config"] = serde_json::json!({ "effort": effort });
            }
        } else if thinking_budget >= crate::provider::thinking::MIN_THINKING_BUDGET {
            // Older Claude models use budget-based thinking. If the remaining
            // context cannot fit the API's minimum budget plus visible output,
            // omit thinking rather than send an invalid request.
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": thinking_budget,
            });
        }
    } else if crate::provider::thinking::thinking_off_disables(config.model_config.as_ref()) {
        // Off explicitly disables thinking rather than omitting the field, so a
        // reasoning model is told not to think this turn instead of falling back
        // to its API default. Mirrors pi's `{ type: "disabled" }`.
        body["thinking"] = serde_json::json!({ "type": "disabled" });
    }

    let supports_temperature = config
        .model_config
        .as_ref()
        .is_none_or(|model| model.supports_temperature);
    if thinking_level == ThinkingLevel::Off && supports_temperature {
        if let Some(temp) = config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
    }

    body
}

fn system_prompt_blocks(prompt: &str, cache_static: bool) -> Vec<serde_json::Value> {
    if prompt.is_empty() {
        return Vec::new();
    }

    let (static_part, dynamic_part) = match prompt.rsplit_once(DYNAMIC_BOUNDARY) {
        Some((static_part, dynamic_part)) => (static_part.trim_end(), dynamic_part.trim_start()),
        None => (prompt, ""),
    };

    let mut blocks = Vec::new();
    if !static_part.is_empty() {
        let mut block = serde_json::json!({
            "type": "text",
            "text": static_part,
        });
        if cache_static {
            block["cache_control"] = serde_json::json!({"type": "ephemeral"});
        }
        blocks.push(block);
    }

    if !dynamic_part.is_empty() {
        blocks.push(serde_json::json!({
            "type": "text",
            "text": dynamic_part,
        }));
    }

    blocks
}

pub fn content_to_anthropic(content: &[Content]) -> Vec<serde_json::Value> {
    content
        .iter()
        .filter(|c| !matches!(c, Content::Text { text } if text.trim().is_empty()))
        .map(|c| match c {
            Content::Text { text } => serde_json::json!({"type": "text", "text": text}),
            Content::Image { .. } => {
                if let Some((data, mime_type)) = c.resolve_image_data() {
                    serde_json::json!({
                        "type": "image",
                        "source": {"type": "base64", "media_type": mime_type, "data": data},
                    })
                } else {
                    serde_json::json!({"type": "text", "text": "[image unavailable]"})
                }
            }
            Content::Thinking {
                thinking,
                metadata: Some(ThinkingMetadata::Anthropic { signature }),
            } if !signature.is_empty() => {
                serde_json::json!({
                    "type": "thinking",
                    "thinking": thinking,
                    "signature": signature,
                })
            }
            // Like pi's transformMessages, unsigned or foreign thinking is
            // visible context but not replayable provider state.
            Content::Thinking { thinking, .. } => serde_json::json!({
                "type": "text",
                "text": thinking,
            }),
            Content::ToolCall {
                id,
                name,
                arguments,
            } => {
                // Sanitise malformed tool_use input that can crash Bedrock's
                // Anthropic-to-Converse translator. When the model produces
                // garbled JSON (e.g. empty-string keys, nested objects where
                // strings are expected) the gateway returns
                // UnknownOperationException. Replace such inputs with an empty
                // object — the original error is already captured in the
                // corresponding tool_result.
                let input = if is_malformed_tool_input(arguments) {
                    serde_json::json!({})
                } else {
                    arguments.clone()
                };
                serde_json::json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                })
            }
        })
        .collect()
}

/// Detect malformed tool_use input that would crash the Bedrock gateway.
///
/// The model occasionally produces garbled JSON under high context pressure —
/// e.g. empty-string keys, nested objects where flat strings are expected.
/// Bedrock's Anthropic-to-Converse translator cannot handle these and returns
/// `UnknownOperationException`.
fn is_malformed_tool_input(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            for (key, _val) in map {
                // Empty-string keys are never valid tool parameters
                if key.is_empty() {
                    return true;
                }
            }
            false
        }
        // input should always be an object
        _ => true,
    }
}

fn is_legacy_empty_response(content: &[Content]) -> bool {
    matches!(content, [Content::Text { text }] if text == "[empty response]")
}

/// Merge consecutive messages that share the same `role`.
///
/// Anthropic requires strict user/assistant alternation. When empty assistant
/// messages are skipped, two user messages can end up adjacent. This function
/// merges their `content` arrays into a single message.
fn merge_consecutive_roles(messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut merged: Vec<serde_json::Value> = Vec::with_capacity(messages.len());
    for msg in messages {
        let same_role = match merged.last() {
            Some(prev) => prev["role"].as_str() == msg["role"].as_str(),
            None => false,
        };
        if same_role {
            // Same role — append content blocks to the previous message
            if let Some(prev) = merged.last_mut() {
                if let (Some(prev_content), Some(new_content)) = (
                    prev["content"].as_array().cloned(),
                    msg["content"].as_array().cloned(),
                ) {
                    let mut combined = prev_content;
                    combined.extend(new_content);
                    prev["content"] = serde_json::json!(combined);
                }
            }
        } else {
            merged.push(msg);
        }
    }
    merged
}
