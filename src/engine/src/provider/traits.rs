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

use super::error::ProviderError;

/// Provider stream outcome.
#[derive(Debug)]
pub enum StreamOutcome {
    /// Stream completed with a normal assistant message.
    Complete(Message),
    /// Stream failed after a tool_use block had started and received input.
    /// The assistant message should be kept for protocol continuity, but the
    /// tool must not be executed; the agent loop should append synthetic error
    /// tool_result blocks.
    ///
    /// This deliberately trades provider-level retry for conversation-level
    /// recovery only after a meaningful partial tool_use exists. Earlier errors
    /// still return [`ProviderError`] so the retry policy can retry normally.
    IncompleteToolUse {
        assistant: Message,
        error: ProviderError,
    },
}

impl StreamOutcome {
    pub fn complete(message: Message) -> Self {
        Self::Complete(message)
    }

    pub fn message(&self) -> &Message {
        match self {
            Self::Complete(message) => message,
            Self::IncompleteToolUse { assistant, .. } => assistant,
        }
    }

    pub fn into_message(self) -> Message {
        match self {
            Self::Complete(message) => message,
            Self::IncompleteToolUse { assistant, .. } => assistant,
        }
    }
}

impl From<Message> for StreamOutcome {
    fn from(message: Message) -> Self {
        Self::Complete(message)
    }
}

/// The core provider trait. Implement this for each LLM backend.
#[async_trait]
pub trait StreamProvider: Send + Sync {
    /// Stream a completion, sending [`StreamEvent`]s through the channel.
    ///
    /// On success returns the assistant stream outcome. Most providers return
    /// [`StreamOutcome::Complete`]; providers with fine-grained tool streaming
    /// may return [`StreamOutcome::IncompleteToolUse`] when the upstream fails
    /// mid-tool-use and the agent can recover by appending an error tool_result.
    /// On failure returns a [`ProviderError`] (used by retry logic to decide
    /// whether the call is retryable).
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError>;
}
