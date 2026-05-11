use napi_derive::napi;

/// Terminate the process immediately via `std::process::exit`, bypassing all
/// Rust `Drop` impls and async runtime shutdown. Used by the CLI on user-
/// triggered exit (Ctrl+C, Ctrl+D, `/exit`) so large sessions don't stall on
/// telemetry flush, tokio runtime teardown, or deallocating accumulated state.
///
/// The caller is responsible for restoring terminal state (raw mode, cursor,
/// bracketed paste) before invoking this.
#[napi(js_name = "fastExit")]
pub fn fast_exit(code: i32) {
    std::process::exit(code);
}
