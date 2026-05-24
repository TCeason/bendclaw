//! Slim core — toggle, types, and entry point.

use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

use super::filter::CmdCtx;
use super::filter::Stream;
use super::router;
use super::stats::SlimStats;

// Tri-state toggle: 0 = default (env var decides), 1 = force on, 2 = force off.
static OVERRIDE: AtomicU8 = AtomicU8::new(0);

/// Session-level override. `Some(true)` = force on, `Some(false)` = force off,
/// `None` = fall back to env var (default on).
pub fn set_enabled_override(value: Option<bool>) {
    let v = match value {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    };
    OVERRIDE.store(v, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    match OVERRIDE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => std::env::var("EVOT_SLIM")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("off") && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true),
    }
}

/// Output of slimming a single tool invocation.
#[derive(Debug, Clone)]
pub struct Slimmed {
    pub stdout: String,
    pub stderr: String,
    pub stats: SlimStats,
}

impl Slimmed {
    fn passthrough(filter: &'static str, stdout: String, stderr: String) -> Self {
        let bytes = stdout.len() + stderr.len();
        Self {
            stats: SlimStats::passthrough(filter, bytes),
            stdout,
            stderr,
        }
    }
}

/// Entry point for `BashTool`.
pub fn on_bash(full_cmd: &str, exit_code: i32, stdout: String, stderr: String) -> Slimmed {
    if !is_enabled() {
        return Slimmed::passthrough("off", stdout, stderr);
    }
    if exit_code != 0 {
        return Slimmed::passthrough("raw_error", stdout, stderr);
    }

    let original = stdout.len() + stderr.len();
    let ctx = CmdCtx::new(full_cmd);

    let stdout_filter = router::pick(&ctx, Stream::Stdout, &stdout);
    let stderr_filter = router::pick(&ctx, Stream::Stderr, &stderr);
    let new_stdout = stdout_filter.apply(&ctx, Stream::Stdout, &stdout);
    let new_stderr = stderr_filter.apply(&ctx, Stream::Stderr, &stderr);

    match (new_stdout, new_stderr) {
        (None, None) => Slimmed::passthrough("none", stdout, stderr),
        (out, err) => {
            let filter_id = if out.is_some() {
                stdout_filter.id()
            } else {
                stderr_filter.id()
            };
            let stdout = out.unwrap_or(stdout);
            let stderr = err.unwrap_or(stderr);
            let slimmed = stdout.len() + stderr.len();
            Slimmed {
                stats: SlimStats::new(filter_id, original, slimmed),
                stdout,
                stderr,
            }
        }
    }
}
