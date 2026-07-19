use std::sync::Arc;
use std::sync::Mutex;

use evot::agent::Agent;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[napi]
pub struct NapiCompaction {
    handle: Mutex<Option<JoinHandle<std::result::Result<String, String>>>>,
    cancel: CancellationToken,
}

impl NapiCompaction {
    pub fn start(
        agent: Arc<Agent>,
        session_id: String,
        custom_instructions: Option<String>,
    ) -> Self {
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            let outcome = agent
                .compact(&session_id, custom_instructions, task_cancel)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_string(&outcome)
                .map_err(|error| format!("serialize compaction: {error}"))
        });
        Self {
            handle: Mutex::new(Some(handle)),
            cancel,
        }
    }
}

#[napi]
impl NapiCompaction {
    #[napi]
    pub async fn result(&self) -> Result<String> {
        let handle = {
            let mut guard = self
                .handle
                .lock()
                .map_err(|_| Error::from_reason("compaction task lock poisoned"))?;
            guard
                .take()
                .ok_or_else(|| Error::from_reason("compaction result already consumed"))?
        };
        handle
            .await
            .map_err(|error| Error::from_reason(format!("compaction task failed: {error}")))?
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn abort(&self) {
        self.cancel.cancel();
    }
}
