use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures::FutureExt;
use tokio::task::JoinHandle;

/// Spawn a named tokio task that catches panics and logs them instead of
/// propagating. Inspired by databend's `spawn_named` + `catch_unwind`.
pub fn spawn_named<F>(name: &'static str, fut: F) -> JoinHandle<()>
where F: Future<Output = ()> + Send + 'static {
    tokio::spawn(async move {
        if let Err(panic) = AssertUnwindSafe(fut).catch_unwind().await {
            let msg = match panic.downcast_ref::<&str>() {
                Some(s) => s.to_string(),
                None => match panic.downcast_ref::<String>() {
                    Some(s) => s.clone(),
                    None => "unknown panic payload".to_string(),
                },
            };
            tracing::error!(task = name, panic = %msg, "task panicked");
        }
    })
}

/// Spawn a fire-and-forget task. The JoinHandle is intentionally dropped.
pub fn spawn_fire_and_forget<F>(name: &'static str, fut: F)
where F: Future<Output = ()> + Send + 'static {
    drop(spawn_named(name, fut));
}
