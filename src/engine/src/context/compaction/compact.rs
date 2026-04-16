//! Compaction orchestration — types, stats, and pipeline assembly.
//!
//! `compact_messages` builds a pipeline of passes and executes them.
//! All implementation logic lives in the `passes/` modules.

use serde::Deserialize;
use serde::Serialize;

use super::pass::CompactionContext;
use super::passes::*;
use super::pipeline::CompactionPipeline;
use super::policy::CompactionPolicy;
use super::sanitize::sanitize_tool_pairs;
use crate::context::tokens::content_tokens;
use crate::context::tokens::total_tokens;
use crate::context::tracking::ContextConfig;
use crate::types::*;

// ---------------------------------------------------------------------------
// Compaction types
// ---------------------------------------------------------------------------

/// Per-tool token breakdown entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolTokenDetail {
    pub tool_name: String,
    pub tokens: usize,
}

/// Describes what happened to a single item during compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionAction {
    /// Message index in the original list (0-based).
    pub index: usize,
    /// Tool name, "assistant", or "messages".
    pub tool_name: String,
    /// What method was used.
    pub method: CompactionMethod,
    /// Tokens before compaction.
    pub before_tokens: usize,
    /// Tokens after compaction.
    pub after_tokens: usize,
    /// End index for range actions (drop).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
    /// Count of related messages (e.g. tool results in a summarized turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_count: Option<usize>,
}

/// The method used to compact a message or tool result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompactionMethod {
    /// Tree-sitter structural outline extraction
    Outline,
    /// Head + tail truncation
    HeadTail,
    /// Turn summarized
    Summarized,
    /// Messages dropped
    Dropped,
    /// CurrentRun result cleared after use
    LifecycleCleared,
    /// Old result cleared by age policy
    #[serde(alias = "age_cleared")]
    AgeCleared,
    /// Oversized result capped
    #[serde(alias = "oversize_capped")]
    OversizeCapped,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionStats {
    /// Highest pass that produced actions: 3=evict, 2=collapse, 1=shrink, 0=clear/no-op
    pub level: u8,
    pub before_message_count: usize,
    pub after_message_count: usize,
    pub before_estimated_tokens: usize,
    pub after_estimated_tokens: usize,
    pub tool_outputs_truncated: usize,
    pub turns_summarized: usize,
    pub messages_dropped: usize,
    pub current_run_cleared: usize,
    /// Count of oversized results capped.
    #[serde(default)]
    pub oversize_capped: usize,
    /// Count of old results cleared by age policy.
    #[serde(default)]
    pub age_cleared: usize,
    /// Per-tool token breakdown before compaction (sorted by tokens desc).
    #[serde(default)]
    pub before_tool_details: Vec<ToolTokenDetail>,
    /// Per-tool token breakdown after compaction (sorted by tokens desc).
    #[serde(default)]
    pub after_tool_details: Vec<ToolTokenDetail>,
    /// Per-message compaction actions.
    #[serde(default)]
    pub actions: Vec<CompactionAction>,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages: Vec<AgentMessage>,
    pub stats: CompactionStats,
}

pub trait CompactionStrategy: Send + Sync {
    fn compact(&self, messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult;
}

pub struct DefaultCompaction;

impl CompactionStrategy for DefaultCompaction {
    fn compact(&self, messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult {
        compact_messages(messages, config)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect per-tool token details from messages, sorted by tokens descending.
fn collect_tool_details(messages: &[AgentMessage]) -> Vec<ToolTokenDetail> {
    let mut details = Vec::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name, content, ..
        }) = msg
        {
            details.push(ToolTokenDetail {
                tool_name: tool_name.clone(),
                tokens: content_tokens(content),
            });
        }
    }
    details.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    details
}

// ---------------------------------------------------------------------------
// Pipeline assembly
// ---------------------------------------------------------------------------

/// Compact messages using a pipeline of passes.
///
/// Pass order:
///   1. `ClearExpiredToolResults`    — always-on: clear `CurrentRun` results
///   2. `ShrinkOversizedToolResults` — always-on: age-evict / oversize-cap / normal-truncate
///   3. `CollapseOldAssistantTurns`  — internally budget-gated: summarize old turns
///   4. `EvictStaleMessages`         — internally budget-gated: drop middle messages
pub fn compact_messages(messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult {
    let budget = config
        .max_context_tokens
        .saturating_sub(config.system_prompt_tokens);

    let ctx = CompactionContext {
        budget,
        keep_recent: config.keep_recent,
        keep_first: config.keep_first,
        tool_output_max_lines: config.tool_output_max_lines,
        policy: CompactionPolicy::default(),
    };

    let before_message_count = messages.len();
    let before_estimated_tokens = total_tokens(&messages);
    let before_tool_details = collect_tool_details(&messages);

    // always-on passes execute first, budget-gated passes check internally
    let pipeline = CompactionPipeline::builder()
        .add(ClearExpiredToolResults) // always-on
        .add(ShrinkOversizedToolResults) // always-on
        .add(CollapseOldAssistantTurns) // internally budget-gated
        .add(EvictStaleMessages) // internally budget-gated
        .build();

    let result = pipeline.execute(messages, &ctx);
    let messages = sanitize_tool_pairs(result.messages);

    let after_message_count = messages.len();
    let after_estimated_tokens = total_tokens(&messages);
    let after_tool_details = collect_tool_details(&messages);

    // Derive counters from actions
    let mut current_run_cleared: usize = 0;
    let mut age_cleared: usize = 0;
    let mut oversize_capped: usize = 0;
    let mut tool_outputs_truncated: usize = 0;
    let mut turns_summarized: usize = 0;
    let mut messages_dropped: usize = 0;

    for action in &result.actions {
        match action.method {
            CompactionMethod::LifecycleCleared => current_run_cleared += 1,
            CompactionMethod::AgeCleared => age_cleared += 1,
            CompactionMethod::OversizeCapped => oversize_capped += 1,
            CompactionMethod::Outline | CompactionMethod::HeadTail => tool_outputs_truncated += 1,
            CompactionMethod::Summarized => turns_summarized += 1,
            CompactionMethod::Dropped => {
                messages_dropped += action.related_count.unwrap_or(1);
            }
        }
    }

    // level = highest pass that produced actions
    let level = if messages_dropped > 0 {
        3
    } else if turns_summarized > 0 {
        2
    } else if tool_outputs_truncated > 0 || oversize_capped > 0 || age_cleared > 0 {
        1
    } else {
        0
    };

    CompactionResult {
        messages,
        stats: CompactionStats {
            level,
            before_message_count,
            after_message_count,
            before_estimated_tokens,
            after_estimated_tokens,
            tool_outputs_truncated,
            turns_summarized,
            messages_dropped,
            current_run_cleared,
            oversize_capped,
            age_cleared,
            before_tool_details,
            after_tool_details,
            actions: result.actions,
        },
    }
}

// ---------------------------------------------------------------------------
// Legacy re-exports for backward compatibility
// ---------------------------------------------------------------------------

/// Re-export `truncate_text_head_tail` from the shrink pass for external use.
pub use super::passes::shrink_oversized::truncate_text_head_tail;
