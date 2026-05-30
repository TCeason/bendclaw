//! SummarizerMode — LLM summarization dispatch.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::llm;
use super::types::SummarizerError;
use super::types::SummarizerInput;
use super::types::SummarizerOutput;
use crate::provider::StreamProvider;
use crate::types::ThinkingLevel;

/// LLM-generated structured summary configuration.
#[derive(Debug, Clone)]
pub enum SummarizerMode {
    Llm { max_tokens: u32 },
}

impl Default for SummarizerMode {
    fn default() -> Self {
        Self::Llm { max_tokens: 4096 }
    }
}

/// Context needed for LLM summarization (passed by caller, not stored).
pub struct SummarizerContext {
    pub provider: Arc<dyn StreamProvider>,
    pub model: String,
    pub api_key: String,
    pub thinking_level: ThinkingLevel,
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
            Self::Llm { max_tokens } => {
                let ctx = ctx.ok_or_else(|| {
                    SummarizerError::Failed("LLM mode requires SummarizerContext".into())
                })?;
                llm::summarize(input, ctx, *max_tokens, cancel).await
            }
        }
    }
}
