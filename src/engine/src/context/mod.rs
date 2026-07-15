//! Context window management — token counting, compaction, and execution limits.

pub mod compaction;
pub mod history;
pub mod image_resize;
pub mod sanitize;
pub mod tokens;
pub mod tracking;

pub use compaction::session::plan_session_compaction;
pub use compaction::session::CompactEntry;
pub use compaction::session::SessionCompactPlan;
pub use compaction::session::SessionSplitTurn;
pub use compaction::truncate_head_tail;
pub use compaction::truncate_summary;
pub use compaction::types::CompactReason;
pub use compaction::AfterResponseAction;
pub use compaction::CompactionConfig;
pub use compaction::CompactionController;
pub use compaction::CompactionOutcome;
pub use compaction::CompactionResponse;
pub use compaction::CompactionState;
pub use compaction::CompactionStats;
pub use compaction::FileOps;
pub use compaction::ModelId;
pub use compaction::SummarizerContext;
pub use compaction::SummarizerInput;
pub use compaction::SummarizerMode;
pub use compaction::TriggerDecision;
pub use compaction::UsageSnapshot;
pub use compaction::DEFAULT_SUMMARY_MAX_BYTES;
pub use history::transform_messages_for_model;
pub use image_resize::resize_image;
pub use sanitize::sanitize_tool_pairs;
pub use tokens::compute_call_stats;
pub use tokens::compute_call_stats_from_agent_messages;
pub use tokens::content_tokens;
pub use tokens::estimate_tokens;
pub use tokens::message_tokens;
pub use tokens::tool_definition_tokens;
pub use tokens::total_tokens;
pub use tracking::ContextBudgetSnapshot;
pub use tracking::ContextConfig;
pub use tracking::ContextTracker;
pub use tracking::ExecutionLimits;
pub use tracking::ExecutionTracker;
pub use tracking::IdleClock;
pub use tracking::IdlePause;

/// Milliseconds since UNIX epoch, or 0 if the system clock is unavailable.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
