//! Context compaction — smart context management for long sessions.
//!
//! Architecture:
//! - `trigger` — decides WHEN to compact (usage-based, overflow detection)
//! - `planner` — decides WHAT to keep/evict (cut point algorithm)
//! - `executor` — applies the plan (reclaim + shrink + evict + memory summary)
//! - `memory` — extracts compacted memory/state from evicted content
//! - `controller` — stateful integration point for the agent loop
//! - `transforms/` — individual message transformations (reclaim, shrink)

pub mod config;
pub mod controller;
pub mod emergency;
pub mod executor;
pub mod memory;
pub mod planner;
pub mod remote;
pub mod session;
pub mod summarizer;
pub mod transforms;
pub mod trigger;
pub mod types;

pub use config::truncate_summary;
pub use config::CompactionConfig;
pub use controller::CompactionController;
pub use controller::CompactionResponse;
pub use summarizer::mode::DEFAULT_SUMMARY_RESERVE_TOKENS;
pub use summarizer::SummarizerContext;
pub use summarizer::SummarizerInput;
pub use summarizer::SummarizerMode;
pub use transforms::shrink::truncate_head_tail;
pub use types::AfterResponseAction;
pub use types::CompactionMethod;
pub use types::CompactionObserver;
pub use types::CompactionOutcome;
pub use types::CompactionPhase;
pub use types::CompactionState;
pub use types::CompactionStats;
pub use types::FileOps;
pub use types::ModelId;
pub use types::TriggerDecision;
pub use types::UsageSnapshot;
