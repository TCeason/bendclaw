use std::sync::Arc;

use super::agent::Agent;
use super::run::runtime;
use super::run::runtime::TurnFactory;
use super::tools::HostTools;
use super::tools::ToolMode;
use crate::conf::LlmConfig;
use crate::error::Result;
use crate::sessions::Session;

pub(super) struct TurnBuildRequest {
    pub input: Vec<evot_engine::Content>,
    pub host_tools: Option<HostTools>,
    pub consume_process_notifications: bool,
}

pub(super) struct AgentTurnFactory {
    pub agent: Arc<Agent>,
    pub session: Arc<Session>,
    pub mode: ToolMode,
    pub session_id: String,
    pub llm: LlmConfig,
    pub host_tools: Option<HostTools>,
}

#[async_trait::async_trait]
impl TurnFactory for AgentTurnFactory {
    async fn build(&self, input: Vec<evot_engine::Content>) -> Result<runtime::TurnInput> {
        self.agent
            .build_turn(
                &self.llm,
                self.mode,
                Arc::clone(&self.session),
                &self.session_id,
                TurnBuildRequest {
                    input,
                    host_tools: self.host_tools.clone(),
                    consume_process_notifications: true,
                },
            )
            .await
    }
}
