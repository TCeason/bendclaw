//! Agent loop integration tests, grouped by concern.

#[path = "run/callbacks.rs"]
mod callbacks;
#[path = "run/common.rs"]
mod common;
#[path = "run/compaction.rs"]
mod compaction;
#[path = "run/core.rs"]
mod core;
#[path = "run/retry.rs"]
mod retry;
#[path = "run/steering.rs"]
mod steering;
#[path = "run/tools.rs"]
mod tools;
