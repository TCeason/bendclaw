//! DB-backed backend for persistent (Cloud + Persistent) sessions.

use std::sync::Arc;

use async_trait::async_trait;

use super::context::SessionContextProvider;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::prompt::PromptConfig;
use crate::kernel::run::usage::UsageScope;
use crate::kernel::session::history_loader::SessionHistoryLoader;
use crate::kernel::Message;

/// Persistent backend — real DB calls for history, quota, and run lifecycle.
pub struct PersistentBackend {
    storage: Arc<AgentStore>,
    session_id: String,
    agent_id: String,
    prompt_config: Option<PromptConfig>,
}

impl PersistentBackend {
    pub fn new(
        storage: Arc<AgentStore>,
        session_id: String,
        agent_id: String,
        prompt_config: Option<PromptConfig>,
    ) -> Self {
        Self {
            storage,
            session_id,
            agent_id,
            prompt_config,
        }
    }
}

#[async_trait]
impl SessionContextProvider for PersistentBackend {
    async fn load_history(&self, limit: usize) -> Result<Vec<Message>> {
        let loader = SessionHistoryLoader::new(self.storage.clone());
        loader.load(&self.session_id, limit as u32).await
    }

    async fn enforce_token_limits(&self) -> Result<()> {
        let config = match &self.prompt_config {
            Some(c) => c.clone(),
            None => return Ok(()),
        };
        let need_total = config.token_limit_total.is_some();
        let need_daily = config.token_limit_daily.is_some();
        if !need_total && !need_daily {
            return Ok(());
        }

        let total_fut = async {
            if need_total {
                Some(
                    self.storage
                        .usage_summarize(UsageScope::AgentTotal {
                            agent_id: self.agent_id.clone(),
                        })
                        .await,
                )
            } else {
                None
            }
        };
        let daily_fut = async {
            if need_daily {
                let day = crate::storage::time::now().date_naive().to_string();
                Some(
                    self.storage
                        .usage_summarize(UsageScope::AgentDaily {
                            agent_id: self.agent_id.clone(),
                            day,
                        })
                        .await,
                )
            } else {
                None
            }
        };

        let (total_result, daily_result) = tokio::join!(total_fut, daily_fut);

        if let (Some(limit), Some(result)) = (config.token_limit_total, total_result) {
            let used = result?.total_tokens;
            if used >= limit {
                return Err(ErrorCode::quota_exceeded(format!(
                    "agent token total limit exceeded: used={used} limit={limit}"
                )));
            }
        }
        if let (Some(limit), Some(result)) = (config.token_limit_daily, daily_result) {
            let used = result?.total_tokens;
            if used >= limit {
                return Err(ErrorCode::quota_exceeded(format!(
                    "agent token daily limit exceeded: used={used} limit={limit}"
                )));
            }
        }
        Ok(())
    }
}
