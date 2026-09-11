//! Spawning tool processes away from evot's controlling terminal.
//!
//! A new process group is not enough: it stays in evot's session and keeps the
//! same tty, so a child can `open("/dev/tty")` and `tcsetpgrp()` the foreground
//! onto itself. evot, now background, is stopped by SIGTTIN on its next read —
//! the shell prints `suspended (tty input)`, which looks like a crash.
//!
//! `setsid()` leaves the session, so there is no tty to take. A session leader
//! is also a group leader, so the pid is still the pgid to signal.
//!
//! Consequence: a command needing its own terminal now fails instead of
//! prompting (`sudo` without a cached credential, `ssh` asking for a
//! passphrase). stdin is already `/dev/null`, so those prompts were unanswerable
//! anyway; the alternative is the hang or the stolen terminal.

use tokio::process::Command;

/// A spawned tool process, killable as a whole group.
pub(crate) struct SpawnedChild {
    #[cfg(unix)]
    child: tokio::process::Child,
    #[cfg(not(unix))]
    child: command_group::AsyncGroupChild,
    pgid: Option<u32>,
}

impl SpawnedChild {
    pub(crate) fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            // SAFETY: runs between fork and exec; `setsid` is async-signal-safe
            // and allocates nothing.
            unsafe {
                command.pre_exec(|| {
                    // Only fails if this pid is already a group leader, which
                    // needs a recycled pid still naming a live group. Failing
                    // the spawn beats a child that can reach evot's terminal.
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let child = command.spawn()?;
            let pgid = child.id();
            Ok(Self { child, pgid })
        }
        #[cfg(not(unix))]
        {
            use command_group::AsyncCommandGroup;
            // Windows has no sessions; the job object already holds the tree.
            let child = command.group_spawn()?;
            let pgid = child.id();
            Ok(Self { child, pgid })
        }
    }

    /// Group id to signal on teardown paths that cannot await.
    pub(crate) fn pgid(&self) -> Option<u32> {
        self.pgid
    }

    pub(crate) fn stdout(&mut self) -> Option<tokio::process::ChildStdout> {
        #[cfg(unix)]
        {
            self.child.stdout.take()
        }
        #[cfg(not(unix))]
        {
            self.child.inner().stdout.take()
        }
    }

    pub(crate) fn stderr(&mut self) -> Option<tokio::process::ChildStderr> {
        #[cfg(unix)]
        {
            self.child.stderr.take()
        }
        #[cfg(not(unix))]
        {
            self.child.inner().stderr.take()
        }
    }

    pub(crate) async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    /// Kill the whole group so a shell's children die too. Does not reap;
    /// callers must still `wait()`.
    pub(crate) async fn kill(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            // The leader can be reaped while its children live on, so a failed
            // group signal still warrants the direct kill.
            let signalled = self.pgid.is_some_and(kill_process_group);
            if !signalled {
                self.child.kill().await?;
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            self.child.kill().await
        }
    }
}

/// Send `SIGKILL` to a whole process group. `false` when the group is already
/// gone, which is expected rather than an error.
#[cfg(unix)]
pub(crate) fn kill_process_group(pgid: u32) -> bool {
    let Ok(pgid) = i32::try_from(pgid) else {
        return false;
    };
    // SAFETY: `killpg` takes a pgid and a signal, with no memory-safety
    // contract. An invalid or reaped group returns -1.
    unsafe { libc::killpg(pgid, libc::SIGKILL) == 0 }
}

#[cfg(not(unix))]
pub(crate) fn kill_process_group(_pgid: u32) -> bool {
    false
}
