//! Reaping every child of the current process ahead of an in-place `execve`.
//!
//! `execve` keeps the pid, so children of the old image (a session-hook
//! adapter still shutting down, background tool processes that were just
//! signalled) are inherited by the new image. Nothing there knows about them:
//! both the JS runtime and tokio only `waitpid` the pids they spawned, so each
//! inherited child turns into a `<defunct>` entry the moment it exits and stays
//! until evot itself exits. Draining the wait queue right before `execve` is
//! the only point where stealing exit statuses from other waiters is harmless,
//! because none of them will ever run again.

use std::time::Duration;
use std::time::Instant;

/// How long to sleep between polls while children are still running.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Reap exited children until none remain or `timeout` elapses.
///
/// Returns the number of children reaped. Children still alive at the deadline
/// are left alone: a persistent adapter that ignores stdin EOF must not hold up
/// the restart, and one leftover zombie is the lesser evil.
pub fn reap_exited_children(timeout: Duration) -> usize {
    let deadline = Instant::now() + timeout;
    let mut reaped = 0;
    loop {
        match wait_any_nonblocking() {
            WaitOutcome::Reaped => reaped += 1,
            WaitOutcome::NoChildren => return reaped,
            WaitOutcome::StillRunning => {
                if Instant::now() >= deadline {
                    return reaped;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

enum WaitOutcome {
    Reaped,
    StillRunning,
    NoChildren,
}

#[cfg(unix)]
fn wait_any_nonblocking() -> WaitOutcome {
    let mut status: libc::c_int = 0;
    // SAFETY: `waitpid` writes the exit status into the provided `c_int` and
    // has no other memory-safety contract. `-1` selects any child; `WNOHANG`
    // makes the call return immediately when none has exited yet.
    let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
    if pid > 0 {
        WaitOutcome::Reaped
    } else if pid == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
        WaitOutcome::StillRunning
    } else {
        // ECHILD: nothing left to wait for.
        WaitOutcome::NoChildren
    }
}

#[cfg(not(unix))]
fn wait_any_nonblocking() -> WaitOutcome {
    WaitOutcome::NoChildren
}
