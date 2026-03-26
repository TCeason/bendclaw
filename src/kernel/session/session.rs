//! Session — the aggregate root for agent conversations.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;

use super::diagnostics;
use super::resources::SessionResources;
use super::run::SessionRunCoordinator;
use super::state::SessionState;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::session::session_manager::SessionInfo;
use crate::kernel::session::session_manager::TurnStats;
use crate::kernel::session::session_stream::Stream;
use crate::kernel::Message;

pub struct Session {
    pub id: String,
    agent_id: Arc<str>,
    user_id: Arc<str>,
    res: SessionResources,
    pub state: Arc<Mutex<SessionState>>,
    history: Arc<Mutex<Vec<Message>>>,
    last_active: Mutex<Instant>,
    stale: AtomicBool,
    queued_followup: Mutex<Option<String>>,
}

impl Session {
    pub fn new(id: String, agent_id: Arc<str>, user_id: Arc<str>, res: SessionResources) -> Self {
        Self {
            id,
            agent_id,
            user_id,
            res,
            state: Arc::new(Mutex::new(SessionState::Idle)),
            history: Arc::new(Mutex::new(Vec::new())),
            last_active: Mutex::new(Instant::now()),
            stale: AtomicBool::new(false),
            queued_followup: Mutex::new(None),
        }
    }

    pub async fn run(
        &self,
        user_message: &str,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
    ) -> Result<Stream> {
        {
            let state = self.state.lock();
            if let SessionState::Running { run_id, .. } = &*state {
                diagnostics::log_run_rejected(&self.id, &self.agent_id, run_id);
                return Err(ErrorCode::denied(format!(
                    "session already has a running run: {run_id}"
                )));
            }
        }
        *self.last_active.lock() = Instant::now();
        let start = Instant::now();

        SessionRunCoordinator {
            session_id: &self.id,
            agent_id: &self.agent_id,
            user_id: &self.user_id,
            resources: &self.res,
            state: &self.state,
            history: &self.history,
        }
        .start(
            user_message,
            trace_id,
            parent_run_id,
            parent_trace_id,
            origin_node_id,
            is_remote_dispatch,
            start,
        )
        .await
    }

    pub async fn chat(&self, user_message: &str, trace_id: &str) -> Result<Stream> {
        self.run(user_message, trace_id, None, "", "", false).await
    }

    /// Inject a user message into the running engine. Returns true if sent.
    pub fn inject_message(&self, msg: &str) -> bool {
        let state = self.state.lock();
        if let SessionState::Running { inbox_tx, .. } = &*state {
            inbox_tx.try_send(Message::user(msg)).is_ok()
        } else {
            false
        }
    }

    pub fn cancel_current(&self) {
        let state = self.state.lock();
        if let SessionState::Running { cancel, .. } = &*state {
            cancel.cancel();
        }
    }

    pub fn cancel_run(&self, run_id: &str) -> bool {
        let state = self.state.lock();
        match &*state {
            SessionState::Running {
                run_id: active_run_id,
                cancel,
                ..
            } if active_run_id == run_id => {
                cancel.cancel();
                true
            }
            _ => false,
        }
    }

    pub fn set_idle(&self) {
        *self.state.lock() = SessionState::Idle;
    }

    pub fn current_run_id(&self) -> Option<String> {
        match &*self.state.lock() {
            SessionState::Running { run_id, .. } => Some(run_id.clone()),
            SessionState::Idle => None,
        }
    }

    pub async fn close(&self) {
        self.cancel_current();
        self.set_idle();
    }

    pub fn is_idle(&self) -> bool {
        matches!(*self.state.lock(), SessionState::Idle)
    }

    pub fn is_running(&self) -> bool {
        matches!(*self.state.lock(), SessionState::Running { .. })
    }

    pub fn idle_duration(&self) -> Duration {
        self.last_active.lock().elapsed()
    }

    pub fn belongs_to(&self, agent_id: &str, user_id: &str) -> bool {
        self.agent_id.as_ref() == agent_id && self.user_id.as_ref() == user_id
    }

    pub fn agent_id_ref(&self) -> &str {
        &self.agent_id
    }

    pub(crate) fn mark_stale(&self) {
        self.stale.store(true, Ordering::Relaxed);
    }

    pub fn is_stale(&self) -> bool {
        self.stale.load(Ordering::Relaxed)
    }

    pub fn queue_followup(&self, input: String) {
        let mut q = self.queued_followup.lock();
        *q = Some(match q.take() {
            Some(existing) if !existing.trim().is_empty() => {
                format!("{existing}\n\n{}", input.trim())
            }
            _ => input,
        });
    }

    pub fn take_followup(&self) -> Option<String> {
        self.queued_followup.lock().take()
    }

    pub fn info(&self) -> SessionInfo {
        let state = self.state.lock();
        let (status, current_turn) = match &*state {
            SessionState::Idle => ("idle".to_string(), None),
            SessionState::Running {
                started_at,
                iteration,
                ..
            } => (
                "running".to_string(),
                Some(TurnStats {
                    iteration: iteration.load(Ordering::Relaxed),
                    duration_ms: started_at.elapsed().as_millis() as u64,
                }),
            ),
        };
        SessionInfo {
            id: self.id.clone(),
            agent_id: self.agent_id.to_string(),
            user_id: self.user_id.to_string(),
            status,
            last_active_ms: self.last_active.lock().elapsed().as_millis() as u64,
            current_turn,
        }
    }
}
