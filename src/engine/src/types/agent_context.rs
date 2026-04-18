use std::path::PathBuf;
use std::sync::Arc;

use super::message::AgentMessage;
use super::tool::AgentTool;
use crate::tools::guard::PathGuard;

// ---------------------------------------------------------------------------
// Agent context (passed to the loop)
// ---------------------------------------------------------------------------

pub struct AgentContext {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Box<dyn AgentTool>>,
    pub cwd: PathBuf,
    pub path_guard: Arc<PathGuard>,
}

// ---------------------------------------------------------------------------
// Input filtering
// ---------------------------------------------------------------------------

/// Result of applying an input filter to a user message.
#[derive(Debug, Clone)]
pub enum FilterResult {
    /// Message passes unchanged.
    Pass,
    /// Message passes, but append a warning to context for the LLM to see.
    Warn(String),
    /// Message is rejected. Agent loop returns immediately.
    Reject(String),
}

/// Synchronous filter applied to user input before the LLM call.
///
/// Implement this for injection detection, content moderation, PII redaction, etc.
/// Filters run in the hot path and must be fast — use `before_turn` callbacks
/// for async moderation (external API calls).
pub trait InputFilter: Send + Sync {
    fn filter(&self, text: &str) -> FilterResult;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
