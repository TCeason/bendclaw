//! Slim — lightweight token-saving post-processing for tool output.
//!
//! Design notes:
//! - One-way dependency: tools → slim → filters; filters are pure.
//! - Global on/off via `EVOT_SLIM=0` env var or process-level toggle.
//! - `exit != 0` commands pass through untouched (`filter = "raw_error"`).
//! - No new `evot.toml` section; thresholds live as module-level consts.

mod core;
pub mod filter;
pub mod filters;
pub mod router;
pub mod stats;

pub use filter::CmdCtx;
pub use filter::Stream;
pub use stats::SlimStats;

pub use self::core::is_enabled;
pub use self::core::on_bash;
pub use self::core::set_enabled_override;
pub use self::core::Slimmed;
