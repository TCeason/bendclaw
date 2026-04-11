use async_trait::async_trait;
use tokio::sync::mpsc;

use super::model::ModelConfig;
use crate::types::*;

/// Events emitted during LLM streaming
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Stream started, partial assistant message
    Start,
    /// Text content delta
    TextDelta { content_index: usize, delta: String },
    /// Thinking content delta
    ThinkingDelta { content_index: usize, delta: String },
    /// Tool call started
    ToolCallStart {
        content_index: usize,
        id: String,
        name: String,
    },
    /// Tool call argument delta
    ToolCallDelta { content_index: usize, delta: String },
    /// Tool call ended
    ToolCallEnd { content_index: usize },
    /// Stream completed successfully
    Done { message: Message },
    /// Stream errored
    Error { message: Message },
}

/// Configuration for a streaming LLM call
#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub model: String,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub thinking_level: ThinkingLevel,
    pub api_key: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    /// Optional model configuration for multi-provider support.
    /// When set, providers use this for base_url, compat flags, headers, etc.
    pub model_config: Option<ModelConfig>,
    /// Prompt caching configuration. Default: enabled with auto strategy.
    pub cache_config: CacheConfig,
}

/// Tool definition sent to the LLM (schema only, no execute fn)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

use serde::Deserialize;
use serde::Serialize;

/// The core provider trait. Implement this for each LLM backend.
#[async_trait]
pub trait StreamProvider: Send + Sync {
    /// Stream a completion, sending [`StreamEvent`]s through the channel.
    ///
    /// On success returns the final complete assistant [`Message`].
    /// On failure returns a [`ProviderError`] (used by retry logic to decide
    /// whether the call is retryable).
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Message, ProviderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("Rate limited, retry after {retry_after_ms:?}ms")]
    RateLimited { retry_after_ms: Option<u64> },
    #[error("Context overflow: {message}")]
    ContextOverflow { message: String },
    #[error("Cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl ProviderError {
    /// Classify an HTTP error response into the appropriate error variant.
    ///
    /// Detects context overflow, rate limits, auth errors, and general API errors
    /// from the HTTP status code and response body.
    pub fn classify(status: u16, message: &str) -> Self {
        if is_context_overflow(status, message) {
            Self::ContextOverflow {
                message: message.to_string(),
            }
        } else if status == 429 {
            Self::RateLimited {
                retry_after_ms: None,
            }
        } else if status == 401 || status == 403 {
            Self::Auth(message.to_string())
        } else if status == 400 || status == 404 || status == 405 || status == 422 {
            // Client errors that won't resolve on retry
            Self::Other(message.to_string())
        } else {
            Self::Api(message.to_string())
        }
    }

    /// Returns true if this error indicates a context overflow.
    pub fn is_context_overflow(&self) -> bool {
        matches!(self, Self::ContextOverflow { .. })
    }
}

/// Extract a classified error from a `reqwest_eventsource::Error`.
///
/// - `InvalidStatusCode` — reads the response body and classifies via
///   [`ProviderError::classify()`] (context overflow, rate limit, auth, etc.).
/// - `InvalidContentType` — reads the JSON body and classifies via
///   [`super::stream_http::classify_json_error()`]. This handles the case where
///   the upstream returns `application/json` instead of `text/event-stream`
///   (e.g. transient errors, overloaded responses).
/// - `Transport` — maps to [`ProviderError::Network`] (retryable).
/// - All other variants (protocol/parse errors like `StreamEnded`,
///   `Utf8`, `Parser`) — maps to [`ProviderError::Other`]
///   (non-retryable, fail fast).
pub async fn classify_eventsource_error(error: reqwest_eventsource::Error) -> ProviderError {
    match error {
        reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
            let status_code = status.as_u16();
            let body = response.text().await.unwrap_or_default();
            ProviderError::classify(
                status_code,
                &format!(
                    "HTTP {} {}: {}",
                    status_code,
                    status.canonical_reason().unwrap_or(""),
                    body
                ),
            )
        }
        reqwest_eventsource::Error::InvalidContentType(_content_type, response) => {
            // The upstream returned a non-SSE content type (typically application/json).
            // Read the body and try to classify the JSON error.
            let body = response.text().await.unwrap_or_default();
            if body.trim().is_empty() {
                ProviderError::Api("Server returned non-SSE content type".into())
            } else {
                match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(value) => super::stream_http::classify_json_error(&value),
                    Err(_) => ProviderError::classify(200, &body),
                }
            }
        }
        reqwest_eventsource::Error::Transport(e) => {
            // Walk the error source chain for more diagnostic context
            let mut detail = e.to_string();
            let mut source = std::error::Error::source(&e);
            while let Some(cause) = source {
                detail.push_str(" -> ");
                detail.push_str(&cause.to_string());
                source = cause.source();
            }
            ProviderError::Network(detail)
        }
        other => ProviderError::Other(other.to_string()),
    }
}

/// Classify an SSE-embedded error event message into a [`ProviderError`].
///
/// Checks the error text for known patterns (context overflow, etc.).
/// Used by providers that receive `"error"` events in the SSE stream.
pub fn classify_sse_error_event(message: &str) -> ProviderError {
    if is_context_overflow_message(message) {
        ProviderError::ContextOverflow {
            message: message.to_string(),
        }
    } else {
        ProviderError::Api(message.to_string())
    }
}

/// Known phrases that indicate context overflow across LLM providers.
///
/// Covers: Anthropic, OpenAI, Google Gemini, AWS Bedrock, xAI, Groq,
/// OpenRouter, llama.cpp, LM Studio, MiniMax, Kimi, GitHub Copilot,
/// and generic patterns.
const OVERFLOW_PHRASES: &[&str] = &[
    "prompt is too long",                 // Anthropic
    "input is too long",                  // AWS Bedrock
    "exceeds the context window",         // OpenAI (Completions & Responses)
    "exceeds the maximum", // Google Gemini ("input token count exceeds the maximum")
    "maximum prompt length", // xAI
    "reduce the length of the messages", // Groq
    "maximum context length", // OpenRouter
    "exceeds the limit of", // GitHub Copilot
    "exceeds the available context size", // llama.cpp
    "greater than the context length", // LM Studio
    "context window exceeds limit", // MiniMax
    "exceeded model token limit", // Kimi
    "context length exceeded", // Generic
    "context_length_exceeded", // Generic (underscore variant)
    "too many tokens",     // Generic
    "token limit exceeded", // Generic
];

/// Check if an error message indicates context overflow (for use by types.rs).
pub fn is_context_overflow_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    OVERFLOW_PHRASES.iter().any(|phrase| lower.contains(phrase))
}

/// Check if an HTTP error response indicates context overflow.
fn is_context_overflow(status: u16, message: &str) -> bool {
    // Some providers (Cerebras, Mistral) return 400/413 with empty body on overflow
    if (status == 400 || status == 413) && message.trim().is_empty() {
        return true;
    }
    is_context_overflow_message(message)
}
