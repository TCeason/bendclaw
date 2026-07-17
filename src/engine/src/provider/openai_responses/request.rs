use crate::provider::traits::StreamConfig;
use crate::types::*;

const MIN_OUTPUT_TOKENS: u32 = 16;

pub fn build_request_body(config: &StreamConfig) -> serde_json::Value {
    let supports_image = config
        .model_config
        .as_ref()
        .map(|model| model.supports_image())
        .unwrap_or(false);
    let reasoning = config
        .model_config
        .as_ref()
        .map(|model| model.reasoning)
        .unwrap_or(true);
    let mut input = Vec::new();

    if !config.system_prompt.is_empty() {
        let role = if reasoning
            && config
                .model_config
                .as_ref()
                .and_then(|model| model.compat.as_ref())
                .is_none_or(|compat| compat.has_cap(crate::provider::CompatCaps::DEVELOPER_ROLE))
        {
            "developer"
        } else {
            "system"
        };
        input.push(serde_json::json!({
            "role": role,
            "content": config.system_prompt,
        }));
    }

    for (message_index, message) in config.messages.iter().enumerate() {
        match message {
            Message::User { content, .. } => {
                let content = input_content(content, supports_image);
                if !content.is_empty() {
                    input.push(serde_json::json!({"role": "user", "content": content}));
                }
            }
            Message::Assistant { content, .. } => {
                for (block_index, block) in content.iter().enumerate() {
                    match block {
                        Content::Text { text } if !text.is_empty() => input.push(serde_json::json!({
                            "type": "message",
                            "id": format!("msg_evot_{message_index}_{block_index}"),
                            "role": "assistant",
                            "status": "completed",
                            "content": [{"type": "output_text", "text": text, "annotations": []}],
                        })),
                        Content::Thinking {
                            metadata: Some(ThinkingMetadata::OpenAiResponses { item }),
                            ..
                        } => input.push(item.clone()),
                        Content::ToolCall { id, name, arguments } => {
                            let (call_id, item_id) = split_tool_call_id(id);
                            let mut item = serde_json::json!({
                                "type": "function_call",
                                "call_id": call_id,
                                "name": name,
                                "arguments": arguments.to_string(),
                            });
                            if let Some(item_id) = item_id {
                                item["id"] = serde_json::json!(item_id);
                            }
                            input.push(item);
                        }
                        // Reasoning from other APIs has no Responses replay item;
                        // omitting it is valid and mirrors pi's cross-provider fallback.
                        _ => {}
                    }
                }
            }
            Message::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                let (call_id, _) = split_tool_call_id(tool_call_id);
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": tool_output(content, supports_image),
                }));
            }
        }
    }

    let mut body = serde_json::json!({
        "model": config.model,
        "input": input,
        "stream": true,
        "store": false,
        "max_output_tokens": config.resolved_max_tokens().max(MIN_OUTPUT_TOKENS),
    });

    if !config.tools.is_empty() {
        body["tools"] = serde_json::Value::Array(
            config
                .tools
                .iter()
                .map(|tool| {
                    serde_json::json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                        "strict": false,
                    })
                })
                .collect(),
        );
    }

    if let Some(key) = &config.prompt_cache_key {
        body["prompt_cache_key"] = serde_json::json!(key);
    }
    if let Some(temperature) = config.temperature {
        body["temperature"] = serde_json::json!(temperature);
    }
    apply_reasoning(&mut body, config, reasoning);
    body
}

fn apply_reasoning(body: &mut serde_json::Value, config: &StreamConfig, enabled: bool) {
    if !enabled {
        return;
    }
    let override_effort = config
        .model_config
        .as_ref()
        .and_then(|model| model.thinking_effort_override(config.thinking_level));
    let effort = override_effort.or(match config.thinking_level {
        ThinkingLevel::Off => config
            .model_config
            .as_ref()
            .filter(|model| model.can_disable_thinking())
            .map(|_| "none"),
        ThinkingLevel::Minimal => Some("minimal"),
        ThinkingLevel::Low => Some("low"),
        ThinkingLevel::Medium => Some("medium"),
        ThinkingLevel::High | ThinkingLevel::Adaptive => Some("high"),
        ThinkingLevel::Xhigh => Some("xhigh"),
        ThinkingLevel::Max => Some("max"),
    });
    if let Some(effort) = effort {
        if effort == "none" {
            body["reasoning"] = serde_json::json!({"effort": effort});
        } else {
            body["reasoning"] = serde_json::json!({"effort": effort, "summary": "auto"});
            body["include"] = serde_json::json!(["reasoning.encrypted_content"]);
        }
    }
}

fn split_tool_call_id(id: &str) -> (&str, Option<&str>) {
    match id.split_once('|') {
        Some((call_id, item_id)) => (call_id, Some(item_id)),
        None => (id, None),
    }
}

fn input_content(content: &[Content], supports_image: bool) -> Vec<serde_json::Value> {
    content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } if !text.is_empty() => {
                Some(serde_json::json!({"type": "input_text", "text": text}))
            }
            Content::Image { .. } if supports_image => {
                block.resolve_image_data().map(|(data, mime_type)| {
                    serde_json::json!({
                        "type": "input_image",
                        "detail": "auto",
                        "image_url": format!("data:{mime_type};base64,{data}"),
                    })
                })
            }
            Content::Image { .. } => Some(serde_json::json!({
                "type": "input_text",
                "text": "[Image omitted: the current model does not support image input.]",
            })),
            _ => None,
        })
        .collect()
}

fn tool_output(content: &[Content], supports_image: bool) -> serde_json::Value {
    let text = content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let images = content
        .iter()
        .filter_map(|block| match block {
            Content::Image { .. } if supports_image => block.resolve_image_data(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if images.is_empty() {
        return serde_json::json!(if text.is_empty() {
            "(no tool output)"
        } else {
            &text
        });
    }

    let mut output = Vec::new();
    if !text.is_empty() {
        output.push(serde_json::json!({"type": "input_text", "text": text}));
    }
    for (data, mime_type) in images {
        output.push(serde_json::json!({
            "type": "input_image",
            "detail": "auto",
            "image_url": format!("data:{mime_type};base64,{data}"),
        }));
    }
    serde_json::Value::Array(output)
}
