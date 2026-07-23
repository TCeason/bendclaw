//! Compaction types — data structures for the compaction system.

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use crate::types::AgentMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactReason {
    Threshold,
    Overflow,
    Manual,
}

/// How the evicted context was summarized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionMethod {
    /// Provider-native server-side compaction (opaque encrypted item).
    Remote,
    /// Local text summarization.
    Local,
    /// Remote compaction was attempted but failed; fell back to local.
    RemoteFailedLocal,
}

/// Live phase shared by automatic and manual compaction frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPhase {
    Planning,
    Remote,
    LocalFallback,
    Local,
    Complete,
}

pub type CompactionObserver = Arc<dyn Fn(CompactionPhase) + Send + Sync>;

pub fn notify_compaction_phase(observer: &Option<CompactionObserver>, phase: CompactionPhase) {
    if let Some(observer) = observer {
        observer(phase);
    }
}

// ---------------------------------------------------------------------------
// Trigger
// ---------------------------------------------------------------------------

/// Identifies a model (provider + model id) for overflow detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelId {
    pub provider: String,
    pub model: String,
}

/// Snapshot of usage from the most recent assistant response.
#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub input: usize,
    pub cache_read: usize,
    pub cache_write: usize,
    pub output: usize,
    pub total_tokens: usize,
    pub model: ModelId,
    pub timestamp: u64,
    pub stop_reason: crate::types::StopReason,
    pub error_message: Option<String>,
}

/// Result of trigger evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerDecision {
    /// No compaction needed.
    Skip,
    /// Context exceeds threshold — compact, do not retry.
    Threshold { context_tokens: usize },
    /// Context overflow error — compact and retry the turn.
    Overflow { context_tokens: usize },
    /// Context overflow detected again after a compact-and-retry was already
    /// attempted this turn. Do not retry — surface a user-visible message so
    /// the user can reduce context or switch to a larger-context model.
    OverflowExhausted { context_tokens: usize },
}

// ---------------------------------------------------------------------------
// Planner
// ---------------------------------------------------------------------------

/// Describes the three zones of a compaction plan.
#[derive(Debug, Clone)]
pub struct CompactionPlan {
    /// Zone A: pinned head messages (always kept).
    pub pinned_head: Range<usize>,
    /// Zone B: messages to evict (replaced by compact memory summary).
    pub evict_zone: Range<usize>,
    /// Zone C: retained tail (recent work, kept in full).
    pub retained_tail: Range<usize>,
    /// If the cut point splits a turn, info about the split.
    pub split_turn: Option<SplitTurn>,
}

/// When the cut point falls inside a turn (not at a user message boundary).
#[derive(Debug, Clone)]
pub struct SplitTurn {
    /// Index of the user message that started this turn.
    pub turn_start: usize,
    /// Index where the retained tail begins (mid-turn).
    pub cut_at: usize,
}

// ---------------------------------------------------------------------------
// Tracker (cross-compaction state)
// ---------------------------------------------------------------------------

/// Accumulated state across compactions — serialized into transcript.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionState {
    /// Cumulative file operations.
    pub file_ops: FileOps,
    /// Environment discoveries (paths, toolchains).
    pub env_discoveries: Vec<String>,
    /// Completed user requests (short summaries).
    pub completed_requests: Vec<String>,
    /// Timestamp of this compaction.
    pub timestamp: u64,
    /// How many compactions have occurred in this session.
    pub generation: u32,
    /// LLM-generated summary from last compaction (for incremental updates).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_summary: Option<String>,
    /// Exact text of the summary message currently present in the LLM context.
    /// Before the next compaction this message is removed and `last_summary` is
    /// supplied to the summarizer instead, preventing duplicate old summaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_summary_message: Option<String>,
}

/// Tracked file operations, accumulated across compactions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileOps {
    pub read: BTreeSet<String>,
    pub written: BTreeSet<String>,
    pub edited: BTreeSet<String>,
}

impl FileOps {
    /// Files that were modified (written or edited).
    pub fn modified(&self) -> BTreeSet<&String> {
        self.written.iter().chain(self.edited.iter()).collect()
    }

    /// Files that were only read (not modified).
    pub fn read_only(&self) -> Vec<&String> {
        let modified = self.modified();
        self.read.iter().filter(|f| !modified.contains(f)).collect()
    }
}

// ---------------------------------------------------------------------------
// Executor output
// ---------------------------------------------------------------------------

/// Final result of a compaction execution.
#[derive(Debug, Clone)]
pub struct CompactionOutcome {
    pub messages: Vec<AgentMessage>,
    pub state: CompactionState,
    pub stats: CompactionStats,
}

/// Statistics about what compaction did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionStats {
    /// Summary generated for the evicted context, when a summarizing compaction ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Messages before compaction.
    pub before_message_count: usize,
    /// Messages after compaction.
    pub after_message_count: usize,
    /// Estimated tokens before.
    pub before_tokens: usize,
    /// Estimated tokens after.
    pub after_tokens: usize,
    /// Messages evicted.
    pub messages_evicted: usize,
    /// CurrentRun results reclaimed.
    pub current_run_reclaimed: usize,
    /// How the summary was produced (None for legacy/no-op stats).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<CompactionMethod>,
    /// Size of the provider-native compaction payload, when remote ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_blob_bytes: Option<usize>,
}

// ---------------------------------------------------------------------------
// Action returned by loop integration
// ---------------------------------------------------------------------------

/// What the agent loop should do after compaction evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterResponseAction {
    /// Continue normally.
    Continue,
    /// Retry the current turn (after overflow recovery).
    Retry,
}
