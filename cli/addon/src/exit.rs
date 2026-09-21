use napi_derive::napi;

/// Terminate the process immediately via `std::process::exit`, bypassing all
/// Rust `Drop` impls and async runtime shutdown. Used by the CLI on user-
/// triggered exit (Ctrl+C, Ctrl+D, `/exit`) so large sessions don't stall on
/// tokio runtime teardown or deallocating accumulated state.
///
/// The caller is responsible for restoring terminal state (raw mode, cursor,
/// bracketed paste) before invoking this.
#[napi(js_name = "fastExit")]
pub fn fast_exit(code: i32) {
    std::process::exit(code);
}

/// Reap every already-exited child of this process, waiting up to
/// `timeout_ms` for the rest to finish. Call immediately before `execve`:
/// the replacement image inherits the pid but never `waitpid`s children it did
/// not spawn, so anything left behind becomes a zombie for its whole lifetime.
#[napi(js_name = "reapExitedChildren")]
pub fn reap_exited_children(timeout_ms: u32) -> u32 {
    let timeout = std::time::Duration::from_millis(u64::from(timeout_ms));
    u32::try_from(evot_engine::tools::process::reap_exited_children(timeout)).unwrap_or(u32::MAX)
}
