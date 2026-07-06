use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use evot::agent::ForkedAgent;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::run::NapiRun;

// ---------------------------------------------------------------------------
// NapiForkedAgent — ephemeral readonly side conversation
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiForkedAgent {
    inner: Arc<Mutex<ForkedAgent>>,
}

#[napi]
impl NapiForkedAgent {
    pub fn new(forked: ForkedAgent) -> Self {
        Self {
            inner: Arc::new(Mutex::new(forked)),
        }
    }

    /// Send a prompt to the forked agent. Returns a NapiRun.
    #[napi]
    pub async fn query(&self, prompt: String) -> Result<NapiRun> {
        let mut forked = self.inner.lock().await;
        let run = forked
            .query(&prompt)
            .await
            .map_err(|e| Error::from_reason(format!("fork query: {e}")))?;
        let sid = run.session_id.clone();
        let handle = run.handle();
        // Forked agents are readonly — no host tools, use dummy channels.
        let (_host_tx, host_rx) = tokio_mpsc::unbounded_channel::<String>();
        Ok(NapiRun {
            inner: Mutex::new(run),
            handle,
            cached_session_id: sid,
            aborted: Arc::new(AtomicBool::new(false)),
            abort_notify: Arc::new(Notify::new()),
            host_event_rx: Mutex::new(Some(host_rx)),
            host_responders: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}
