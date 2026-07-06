use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::model::ModelConfig;
use crate::provider::ProviderError;
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
    /// Optional key for provider-side prompt cache routing.
    pub prompt_cache_key: Option<String>,
}

/// Headroom kept between the request's input tokens and the context window when
/// clamping the output budget, so the model always has room to respond. Matches
/// pi's `CONTEXT_SAFETY_TOKENS`.
const CONTEXT_SAFETY_TOKENS: usize = 4096;

impl StreamConfig {
    /// The output-token budget to send this request, clamped to what the
    /// context window can still hold.
    ///
    /// Starts from the caller's explicit `max_tokens`, else the model's default
    /// cap, else a conservative floor. Then — mirroring pi's
    /// `clampMaxTokensToContext` — caps it to `context_window - input - safety`
    /// so a generous model cap never overflows the window (which providers
    /// reject) or over-reserves credit on metered keys. Never returns 0.
    pub fn resolved_max_tokens(&self) -> u32 {
        let requested = self
            .max_tokens
            .or(self.model_config.as_ref().map(|m| m.max_tokens))
            .unwrap_or(8192);

        let context_window = self
            .model_config
            .as_ref()
            .map(|m| m.context_window)
            .unwrap_or(0);
        if context_window == 0 {
            return requested.max(1);
        }

        let input_tokens = crate::context::estimate_tokens(&self.system_prompt)
            + crate::context::tool_definition_tokens(&self.tools)
            + self
                .messages
                .iter()
                .map(estimate_message_tokens)
                .sum::<usize>();
        let available = (context_window as usize)
            .saturating_sub(input_tokens)
            .saturating_sub(CONTEXT_SAFETY_TOKENS)
            .max(1);
        (requested as usize).min(available) as u32
    }
}

/// Token estimate for an LLM `Message`, reusing the shared content heuristic.
fn estimate_message_tokens(msg: &Message) -> usize {
    let content = match msg {
        Message::User { content, .. } => content,
        Message::Assistant { content, .. } => content,
        Message::ToolResult { content, .. } => content,
    };
    crate::context::content_tokens(content) + 4
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

/// Provider stream outcome.
#[derive(Debug)]
pub struct StreamOutcome {
    message: Message,
}

impl StreamOutcome {
    pub fn complete(message: Message) -> Self {
        Self { message }
    }

    pub fn message(&self) -> &Message {
        &self.message
    }

    pub fn into_message(self) -> Message {
        self.message
    }
}

impl From<Message> for StreamOutcome {
    fn from(message: Message) -> Self {
        Self::complete(message)
    }
}

/// The core provider trait. Implement this for each LLM backend.
#[async_trait]
impl<T: StreamProvider + ?Sized> StreamProvider for Arc<T> {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        self.as_ref().stream(config, tx, cancel).await
    }
}

#[async_trait]
pub trait StreamProvider: Send + Sync {
    /// Stream a completion, sending [`StreamEvent`]s through the channel.
    ///
    /// On success returns the completed assistant message. On failure returns a
    /// [`ProviderError`] for retry/error handling.
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError>;
}
