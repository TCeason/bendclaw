//! SummarizerMode — LLM summarization dispatch.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::llm;
use super::types::SummarizerError;
use super::types::SummarizerInput;
use super::types::SummarizerOutput;
use crate::provider::StreamProvider;
use crate::types::ThinkingLevel;

pub const DEFAULT_SUMMARY_RESERVE_TOKENS: u32 = 16_384;

/// LLM-generated structured summary configuration.
#[derive(Debug, Clone)]
pub enum SummarizerMode {
    Llm { reserve_tokens: u32 },
}

impl Default for SummarizerMode {
    fn default() -> Self {
        Self::Llm {
            reserve_tokens: DEFAULT_SUMMARY_RESERVE_TOKENS,
        }
    }
}

/// Context needed for LLM summarization (passed by caller, not stored).
pub struct SummarizerContext {
    pub provider: Arc<dyn StreamProvider>,
    pub model: String,
    pub api_key: String,
    pub thinking_level: ThinkingLevel,
    /// Normal-turn request shape. Remote compaction mirrors these fields so
    /// provider-native state includes the same instructions/tools/settings.
    pub system_prompt: String,
    pub tools: Vec<crate::provider::ToolDefinition>,
    pub max_tokens: Option<u32>,
    pub cache_config: crate::types::CacheConfig,
    pub prompt_cache_key: Option<String>,
    /// Full transport/provider metadata for the summarization request. Without
    /// this, custom Anthropic-compatible channels silently fall back to the
    /// default Anthropic base URL and compaction fails before the main request.
    pub model_config: Option<crate::provider::ModelConfig>,
}

impl SummarizerMode {
    /// Generate a summary for threshold/manual compaction.
    pub async fn summarize(
        &self,
        input: SummarizerInput,
        ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> Result<SummarizerOutput, SummarizerError> {
        match self {
            Self::Llm { reserve_tokens } => {
                let ctx = ctx.ok_or_else(|| {
                    SummarizerError::Failed("LLM mode requires SummarizerContext".into())
                })?;
                llm::summarize(input, ctx, *reserve_tokens, cancel).await
            }
        }
    }
}
