use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::process::AgentProcess;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentStateKey {
    agent_type: String,
    working_dir: PathBuf,
}

impl AgentStateKey {
    pub fn new(agent_type: impl Into<String>, working_dir: impl Into<PathBuf>) -> Self {
        Self {
            agent_type: agent_type.into(),
            working_dir: working_dir.into(),
        }
    }
}

#[derive(Default)]
struct AgentRuntimeState {
    session_id: Option<String>,
    followup_process: Option<AgentProcess>,
}

/// Session-level state for CLI agent processes.
pub struct CliAgentState {
    agents: HashMap<AgentStateKey, AgentRuntimeState>,
}

impl CliAgentState {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    pub fn get_session_id(&self, key: &AgentStateKey) -> Option<&str> {
        self.agents.get(key)?.session_id.as_deref()
    }

    pub fn set_session_id(&mut self, key: AgentStateKey, session_id: String) {
        self.agents.entry(key).or_default().session_id = Some(session_id);
    }

    pub fn take_followup_process(&mut self, key: &AgentStateKey) -> Option<AgentProcess> {
        self.agents.get_mut(key)?.followup_process.take()
    }

    pub fn set_followup_process(&mut self, key: AgentStateKey, process: AgentProcess) {
        self.agents.entry(key).or_default().followup_process = Some(process);
    }

    pub fn has_followup_process(&self, key: &AgentStateKey) -> bool {
        self.agents
            .get(key)
            .and_then(|state| state.followup_process.as_ref())
            .is_some()
    }
}

impl Default for CliAgentState {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedAgentState = Arc<Mutex<CliAgentState>>;

pub fn new_shared_state() -> SharedAgentState {
    Arc::new(Mutex::new(CliAgentState::new()))
}
