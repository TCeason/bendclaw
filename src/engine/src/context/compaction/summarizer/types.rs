//! Types for the summarizer subsystem.

use crate::context::compaction::types::FileOps;

/// Pre-processed input for summarization (all owned, no lifetimes).
#[derive(Debug, Clone)]
pub struct SummarizerInput {
    /// Serialized conversation text of evicted messages.
    pub conversation: String,
    /// Serialized turn prefix text (if split turn).
    pub turn_prefix: Option<String>,
    /// Previous LLM summary for incremental update.
    pub previous_summary: Option<String>,
    /// File operations extracted from evicted zone (rule-based, always present).
    pub file_ops: FileOps,
    /// Number of evicted messages (for summary header).
    pub evicted_count: usize,
    /// Completed user requests extracted from evicted zone.
    pub completed_requests: Vec<String>,
    /// Environment discoveries extracted from evicted zone.
    pub env_discoveries: Vec<String>,
    /// Last assistant conclusion text from evicted zone.
    pub last_conclusion: Option<String>,
}

/// Output from summarization.
#[derive(Debug, Clone)]
pub struct SummarizerOutput {
    /// The summary text (becomes the memory summary message content).
    pub summary: String,
}

/// Errors from LLM summarization.
#[derive(Debug, Clone)]
pub enum SummarizerError {
    /// LLM call failed.
    Failed(String),
    /// Cancelled by user.
    Cancelled,
}

impl std::fmt::Display for SummarizerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(msg) => write!(f, "summarization failed: {msg}"),
            Self::Cancelled => write!(f, "summarization cancelled"),
        }
    }
}
