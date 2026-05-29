//! SummarizerMode — enum dispatch for rule-based vs LLM summarization.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::llm;
use super::rule_based;
use super::types::SummarizerError;
use super::types::SummarizerInput;
use super::types::SummarizerOutput;
use crate::provider::StreamProvider;
use crate::types::ThinkingLevel;

/// Which summarization strategy to use for marker generation.
#[derive(Debug, Clone, Default)]
pub enum SummarizerMode {
    /// Rule-based extraction (default, zero cost, sync).
    #[default]
    RuleBased,
    /// LLM-generated structured summary (async, costs tokens).
    Llm { max_tokens: u32 },
}

/// Context needed for LLM summarization (passed by caller, not stored).
pub struct SummarizerContext {
    pub provider: Arc<dyn StreamProvider>,
    pub model: String,
    pub api_key: String,
    pub thinking_level: ThinkingLevel,
}

impl SummarizerMode {
    /// Generate a summary for threshold compaction.
    /// For RuleBased, `ctx` is ignored.
    /// For Llm, `ctx` must be Some.
    pub async fn summarize(
        &self,
        input: SummarizerInput,
        ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> Result<SummarizerOutput, SummarizerError> {
        match self {
            Self::RuleBased => Ok(rule_based::summarize(&input)),
            Self::Llm { max_tokens } => {
                let ctx = ctx.ok_or_else(|| {
                    SummarizerError::Failed("LLM mode requires SummarizerContext".into())
                })?;
                llm::summarize(input, ctx, *max_tokens, cancel).await
            }
        }
    }

    /// Sync rule-based summarization. Used for overflow (always fast).
    pub fn summarize_rule_based(&self, input: &SummarizerInput) -> SummarizerOutput {
        rule_based::summarize(input)
    }
}
