use std::collections::HashMap;
use std::time::Instant;

use async_trait::async_trait;
use bend_base::logx;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

use super::provider::ApiType;
use super::provider::LLMProvider;
use super::provider::ProviderRequest;
use super::provider::ProviderResponse;
use super::response;
use super::ApiError;
use crate::types::ContentBlock;
use crate::types::Message;
use crate::types::MessageRole;
use crate::types::Usage;

const DEFAULT_BASE_URL: &str = "https://api.openai.com";

// --- OpenAI request types ---

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    max_tokens: u64,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Serialize)]
struct OpenAIFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

/// OpenAI Chat Completions API provider.
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    base_url: String,
    custom_headers: HashMap<String, String>,
}

impl OpenAIProvider {
    pub fn new(
        client: Client,
        api_key: String,
        base_url: Option<String>,
        custom_headers: HashMap<String, String>,
    ) -> Self {
        Self {
            client,
            api_key,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            custom_headers,
        }
    }

    fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    fn api_type(&self) -> ApiType {
        ApiType::OpenAICompletions
    }

    async fn create_message(
        &self,
        request: ProviderRequest<'_>,
    ) -> Result<ProviderResponse, ApiError> {
        let started_at = Instant::now();
        logx!(
            info,
            "llm",
            "request",
            provider = "openai",
            model = %request.model,
            message_count = request.messages.len() as u64,
            tool_count = request.tools.as_ref().map(|tools| tools.len()).unwrap_or(0) as u64,
            max_tokens = request.max_tokens,
        );

        // Convert messages to OpenAI format
        let mut openai_messages = Vec::new();

        // System prompt as first message
        if let Some(system_blocks) = &request.system {
            let system_text: String = system_blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            if !system_text.is_empty() {
                openai_messages.push(OpenAIMessage {
                    role: "system".to_string(),
                    content: Some(Value::String(system_text)),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }

        // Convert conversation messages
        for msg in request.messages {
            match msg.role {
                MessageRole::User => {
                    // Check if this is a tool_result message
                    let tool_results: Vec<&ContentBlock> = msg
                        .content
                        .iter()
                        .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                        .collect();

                    if !tool_results.is_empty() {
                        // Each tool_result becomes a separate "tool" role message
                        for block in &tool_results {
                            if let ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } = block
                            {
                                let text: String = content
                                    .iter()
                                    .filter_map(|c| match c {
                                        crate::types::ToolResultContentBlock::Text { text } => {
                                            Some(text.as_str())
                                        }
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                openai_messages.push(OpenAIMessage {
                                    role: "tool".to_string(),
                                    content: Some(Value::String(text)),
                                    tool_calls: None,
                                    tool_call_id: Some(tool_use_id.clone()),
                                });
                            }
                        }
                    } else {
                        // Regular user message
                        let text = crate::types::extract_text(msg);
                        openai_messages.push(OpenAIMessage {
                            role: "user".to_string(),
                            content: Some(Value::String(text)),
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }
                }
                MessageRole::Assistant => {
                    let text = crate::types::extract_text(msg);
                    let tool_uses: Vec<OpenAIToolCall> = msg
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::ToolUse { id, name, input } => Some(OpenAIToolCall {
                                id: id.clone(),
                                call_type: "function".to_string(),
                                function: OpenAIFunctionCall {
                                    name: name.clone(),
                                    arguments: serde_json::to_string(input).unwrap_or_default(),
                                },
                            }),
                            _ => None,
                        })
                        .collect();

                    openai_messages.push(OpenAIMessage {
                        role: "assistant".to_string(),
                        content: if text.is_empty() {
                            None
                        } else {
                            Some(Value::String(text))
                        },
                        tool_calls: if tool_uses.is_empty() {
                            None
                        } else {
                            Some(tool_uses)
                        },
                        tool_call_id: None,
                    });
                }
            }
        }

        // Convert tools to OpenAI format
        let openai_tools: Option<Vec<OpenAITool>> = request.tools.map(|tools| {
            tools
                .into_iter()
                .map(|t| OpenAITool {
                    tool_type: "function".to_string(),
                    function: OpenAIFunction {
                        name: t.name,
                        description: t.description,
                        parameters: t.input_schema,
                    },
                })
                .collect()
        });

        let body = OpenAIRequest {
            model: request.model.to_string(),
            max_tokens: request.max_tokens,
            messages: openai_messages,
            tools: openai_tools.filter(|t| !t.is_empty()),
            stream: true,
        };

        let mut req_builder = self
            .client
            .post(self.chat_completions_url())
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json");

        for (key, value) in &self.custom_headers {
            req_builder = req_builder.header(key, value);
        }

        let response = match req_builder.json(&body).send().await {
            Ok(response) => response,
            Err(error) => {
                let api_error = if error.is_timeout() {
                    ApiError::Timeout
                } else {
                    ApiError::NetworkError(error.to_string())
                };
                logx!(
                    warn,
                    "llm",
                    "request_failed",
                    provider = "openai",
                    model = %request.model,
                    error = %api_error,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                );
                return Err(api_error);
            }
        };

        if !response.status().is_success() {
            let error = response::http_error(response).await;
            logx!(
                warn,
                "llm",
                "response_failed",
                provider = "openai",
                model = %request.model,
                error = %error,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
            );
            return Err(error);
        }

        let provider_response = match parse_openai_stream(response).await {
            Ok(provider_response) => provider_response,
            Err(error) => {
                logx!(
                    warn,
                    "llm",
                    "stream_failed",
                    provider = "openai",
                    model = %request.model,
                    error = %error,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                );
                return Err(error);
            }
        };

        logx!(
            info,
            "llm",
            "completed",
            provider = "openai",
            model = %request.model,
            input_tokens = provider_response.usage.input_tokens,
            output_tokens = provider_response.usage.output_tokens,
            stop_reason = %provider_response.stop_reason.clone().unwrap_or_default(),
            elapsed_ms = started_at.elapsed().as_millis() as u64,
        );

        Ok(provider_response)
    }
}

/// Parse OpenAI SSE stream into a ProviderResponse.
async fn parse_openai_stream(response: reqwest::Response) -> Result<ProviderResponse, ApiError> {
    let content_type = response::response_content_type(response.headers());
    let body = response
        .text()
        .await
        .map_err(|e| ApiError::NetworkError(e.to_string()))?;

    if !response::is_streaming_content_type(&content_type) || !response::has_sse_data_lines(&body) {
        return parse_openai_fallback(&body);
    }

    let mut text_content = String::new();
    let mut tool_calls: HashMap<usize, OpenAIToolCall> = HashMap::new();
    let mut usage = Usage::default();
    let mut stop_reason: Option<String> = None;
    let mut saw_valid_sse_event = false;

    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with("data: ") {
            continue;
        }
        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }

        let chunk: Value = match serde_json::from_str(data) {
            Ok(chunk) => {
                saw_valid_sse_event = true;
                chunk
            }
            Err(_) => continue,
        };

        if let Some(error) = response::stream_error(&chunk) {
            return Err(error);
        }

        // Parse usage if present
        if let Some(u) = chunk.get("usage") {
            if let Some(pt) = u.get("prompt_tokens").and_then(|v| v.as_u64()) {
                usage.input_tokens = pt;
            }
            if let Some(ct) = u.get("completion_tokens").and_then(|v| v.as_u64()) {
                usage.output_tokens = ct;
            }
        }

        // Parse choices
        if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                // Check finish_reason
                if let Some(fr) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    stop_reason = Some(match fr {
                        "stop" => "end_turn".to_string(),
                        "tool_calls" => "tool_use".to_string(),
                        "length" => "max_tokens".to_string(),
                        other => other.to_string(),
                    });
                }

                let delta = match choice.get("delta") {
                    Some(d) => d,
                    None => continue,
                };

                // Text content
                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                    text_content.push_str(content);
                }

                // Tool calls
                if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tcs {
                        let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                        let entry = tool_calls.entry(idx).or_insert_with(|| OpenAIToolCall {
                            id: String::new(),
                            call_type: "function".to_string(),
                            function: OpenAIFunctionCall {
                                name: String::new(),
                                arguments: String::new(),
                            },
                        });

                        if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                            entry.id = id.to_string();
                        }
                        if let Some(func) = tc.get("function") {
                            if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                                entry.function.name = name.to_string();
                            }
                            if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                                entry.function.arguments.push_str(args);
                            }
                        }
                    }
                }
            }
        }
    }

    if !saw_valid_sse_event {
        return parse_openai_fallback(&body);
    }

    // Build normalized content blocks
    let mut content_blocks: Vec<ContentBlock> = Vec::new();

    if !text_content.is_empty() {
        content_blocks.push(ContentBlock::Text { text: text_content });
    }

    // Convert tool calls to ContentBlock::ToolUse
    let mut sorted_calls: Vec<(usize, OpenAIToolCall)> = tool_calls.into_iter().collect();
    sorted_calls.sort_by_key(|(idx, _)| *idx);
    for (_, tc) in sorted_calls {
        let input: Value = serde_json::from_str(&tc.function.arguments)
            .unwrap_or(Value::Object(serde_json::Map::new()));
        content_blocks.push(ContentBlock::ToolUse {
            id: if tc.id.is_empty() {
                format!("call_{}", uuid::Uuid::new_v4())
            } else {
                tc.id
            },
            name: tc.function.name,
            input,
        });
    }

    Ok(ProviderResponse {
        message: Message {
            role: MessageRole::Assistant,
            content: content_blocks,
        },
        usage,
        stop_reason,
    })
}

fn parse_openai_fallback(body: &str) -> Result<ProviderResponse, ApiError> {
    let value = response::parse_json_body(body, "OpenAI")?;
    if let Some(error) = response::json_stream_error(&value, "OpenAI") {
        return Err(error);
    }
    parse_openai_response(&value)
}

fn parse_openai_response(value: &Value) -> Result<ProviderResponse, ApiError> {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| {
            ApiError::StreamError("OpenAI upstream returned JSON body without choices".to_string())
        })?;

    let message = choice.get("message").ok_or_else(|| {
        ApiError::StreamError("OpenAI upstream returned JSON body without message".to_string())
    })?;

    let mut content_blocks = Vec::new();

    if let Some(content) = message.get("content") {
        if let Some(text) = content.as_str() {
            if !text.is_empty() {
                content_blocks.push(ContentBlock::Text {
                    text: text.to_string(),
                });
            }
        } else if let Some(parts) = content.as_array() {
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        content_blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
            }
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let id = tool_call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let function = match tool_call.get("function") {
                Some(function) => function,
                None => continue,
            };
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let input = parse_openai_tool_arguments(function.get("arguments"));

            content_blocks.push(ContentBlock::ToolUse { id, name, input });
        }
    }

    let usage = parse_openai_usage(value.get("usage"));
    let stop_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(map_openai_stop_reason);

    Ok(ProviderResponse {
        message: Message {
            role: MessageRole::Assistant,
            content: content_blocks,
        },
        usage,
        stop_reason,
    })
}

fn parse_openai_usage(value: Option<&Value>) -> Usage {
    let mut usage = Usage::default();

    if let Some(value) = value {
        if let Some(prompt_tokens) = value.get("prompt_tokens").and_then(Value::as_u64) {
            usage.input_tokens = prompt_tokens;
        }
        if let Some(completion_tokens) = value.get("completion_tokens").and_then(Value::as_u64) {
            usage.output_tokens = completion_tokens;
        }
    }

    usage
}

fn map_openai_stop_reason(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        "length" => "max_tokens".to_string(),
        other => other.to_string(),
    }
}

fn parse_openai_tool_arguments(arguments: Option<&Value>) -> Value {
    match arguments {
        Some(Value::String(arguments)) => match serde_json::from_str(arguments) {
            Ok(value) => value,
            Err(_) => Value::Object(serde_json::Map::new()),
        },
        Some(value) if value.is_object() || value.is_array() => value.clone(),
        _ => Value::Object(serde_json::Map::new()),
    }
}
