//! Channel conversation routing — serializes per-conversation to prevent
//! duplicate runs. Session identity is derived from `SessionLocator`.
//!
//! Direct session APIs (HTTP, NAPI) bypass this and call Agent directly.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex as SyncMutex;
use tokio::sync::Mutex as AsyncMutex;

use super::run::run::Run;
use super::session::Session;
use super::session_locator::SessionLocator;
use super::Agent;
use super::QueryRequest;
use crate::agent::SubmitOutcome;
use crate::error::Result;

pub enum SendOutcome {
    Started(Run),
    Steered,
    /// A gateway command was handled; carry this text back to the user.
    Command(String),
}

pub struct RunManager {
    agent: Arc<Agent>,
    /// Per-conversation serialization gates keyed by `SessionLocator::stable_key()`.
    gates: SyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl RunManager {
    pub fn new(agent: Arc<Agent>) -> Arc<Self> {
        Arc::new(Self {
            agent,
            gates: SyncMutex::new(HashMap::new()),
        })
    }

    pub fn agent(&self) -> &Arc<Agent> {
        &self.agent
    }

    pub async fn send(
        &self,
        locator: &SessionLocator,
        request: QueryRequest,
    ) -> Result<SendOutcome> {
        let key = locator.stable_key();

        // Acquire per-conversation gate
        let gate = {
            let mut gates = self.gates.lock();
            gates
                .entry(key.clone())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        let _guard = gate.lock().await;

        // Resolve session via locator (open existing or create new)
        let session = Session::open_or_create(
            locator,
            self.agent.cwd(),
            &self.agent.llm().model,
            self.agent.storage(),
        )
        .await?;

        let session_id = session.session_id().await;

        // Steer into active run if one exists
        if self.agent.has_active_run(&session_id) {
            self.agent.steer(&session_id, request.input.clone());
            return Ok(SendOutcome::Steered);
        }

        // Start new run (commands are intercepted inside Agent)
        match self.agent.submit_to_session(request, session).await? {
            SubmitOutcome::Run(run) => Ok(SendOutcome::Started(run)),
            SubmitOutcome::Command(msg) => Ok(SendOutcome::Command(msg)),
        }
    }
}
