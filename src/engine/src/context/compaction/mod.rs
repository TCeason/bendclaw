//! Context compaction — smart context management for long sessions.
//!
//! Architecture:
//! - `trigger` — decides WHEN to compact (usage-based, overflow detection)
//! - `planner` — decides WHAT to keep/evict (cut point algorithm)
//! - `executor` — applies the plan (reclaim + shrink + evict + marker)
//! - `marker` — generates structured summary for evicted content
//! - `controller` — stateful integration point for the agent loop
//! - `transforms/` — individual message transformations (reclaim, shrink)

pub mod config;
pub mod controller;
pub mod executor;
pub mod marker;
pub mod planner;
pub mod summarizer;
pub mod transforms;
pub mod trigger;
pub mod types;

pub use config::CompactionConfig;
pub use controller::CompactionController;
pub use controller::CompactionResponse;
pub use summarizer::SummarizerContext;
pub use summarizer::SummarizerMode;
pub use transforms::shrink::truncate_head_tail;
pub use types::AfterResponseAction;
pub use types::CompactionOutcome;
pub use types::CompactionState;
pub use types::CompactionStats;
pub use types::FileOps;
pub use types::ModelId;
pub use types::TriggerDecision;
pub use types::UsageSnapshot;
