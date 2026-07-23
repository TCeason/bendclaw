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
    phase: Arc<Mutex<String>>,
}

impl NapiCompaction {
    pub fn start(
        agent: Arc<Agent>,
        session_id: String,
        custom_instructions: Option<String>,
    ) -> Self {
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let phase = Arc::new(Mutex::new("planning".to_string()));
        let observer_phase = Arc::clone(&phase);
        let observer: evot::compact::orchestrator::ManualCompactionObserver =
            Arc::new(move |next| {
                let value = match next {
                    evot::compact::orchestrator::ManualCompactionPhase::Planning => "planning",
                    evot::compact::orchestrator::ManualCompactionPhase::Remote => "remote",
                    evot::compact::orchestrator::ManualCompactionPhase::LocalFallback => {
                        "local_fallback"
                    }
                    evot::compact::orchestrator::ManualCompactionPhase::Local => "local",
                    evot::compact::orchestrator::ManualCompactionPhase::Complete => "complete",
                };
                if let Ok(mut current) = observer_phase.lock() {
                    *current = value.to_string();
                }
            });
        let handle = tokio::spawn(async move {
            let outcome = agent
                .compact_with_observer(
                    &session_id,
                    custom_instructions,
                    task_cancel,
                    Some(observer),
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_string(&outcome)
                .map_err(|error| format!("serialize compaction: {error}"))
        });
        Self {
            handle: Mutex::new(Some(handle)),
            cancel,
            phase,
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

    #[napi(getter)]
    pub fn phase(&self) -> Result<String> {
        self.phase
            .lock()
            .map(|phase| phase.clone())
            .map_err(|_| Error::from_reason("compaction phase lock poisoned"))
    }

    #[napi]
    pub fn abort(&self) {
        self.cancel.cancel();
    }
}
