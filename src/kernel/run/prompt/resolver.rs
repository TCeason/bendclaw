//! PromptResolver — per-invocation prompt resolution.
//!
//! LocalPromptResolver: calls build_prompt() with seed. No DB.
//! CloudPromptResolver: calls CloudPromptLoader::build(). DB-backed.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::prompt::build::build_prompt;
use crate::kernel::run::prompt::loader::CloudPromptLoader;
use crate::kernel::run::prompt::model::*;
use crate::kernel::runtime::org::OrgServices;
use crate::llm::tool::ToolSchema;

/// Resolves the final system prompt per invocation.
#[async_trait]
pub trait PromptResolver: Send + Sync {
    async fn resolve(&self, meta: &PromptRequestMeta) -> Result<String>;
}

/// Local: calls build_prompt() with seed + meta overlays. No DB.
pub struct LocalPromptResolver {
    seed: PromptSeed,
    tools: Arc<Vec<ToolSchema>>,
    cwd: PathBuf,
}

impl LocalPromptResolver {
    pub fn new(seed: PromptSeed, tools: Arc<Vec<ToolSchema>>, cwd: PathBuf) -> Self {
        Self { seed, tools, cwd }
    }
}

#[async_trait]
impl PromptResolver for LocalPromptResolver {
    async fn resolve(&self, meta: &PromptRequestMeta) -> Result<String> {
        Ok(build_prompt(PromptInputs {
            seed: self.seed.clone(),
            tools: self.tools.clone(),
            cwd: self.cwd.clone(),
            system_overlay: meta.system_overlay.clone(),
            skill_overlay: meta.skill_overlay.clone(),
            memory_recall: None,
            cluster_info: None,
            recent_errors: None,
            session_state: None,
            channel_type: meta.channel_type.clone(),
            channel_chat_id: meta.channel_chat_id.clone(),
            runtime_override: None,
        }))
    }
}

/// Cloud: delegates to CloudPromptLoader for DB-backed prompt resolution.
pub struct CloudPromptResolver {
    storage: Arc<AgentStore>,
    org: Arc<OrgServices>,
    tools: Arc<Vec<ToolSchema>>,
    variables: Vec<PromptVariable>,
    prompt_config: Option<PromptConfig>,
    cwd: PathBuf,
    cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
    directive: Option<Arc<crate::kernel::directive::DirectiveService>>,
    memory_enabled: bool,
    memory_recall_budget: usize,
    agent_id: String,
    user_id: String,
    session_id: String,
}

impl CloudPromptResolver {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: Arc<AgentStore>,
        org: Arc<OrgServices>,
        tools: Arc<Vec<ToolSchema>>,
        variables: Vec<PromptVariable>,
        prompt_config: Option<PromptConfig>,
        cwd: PathBuf,
        cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
        directive: Option<Arc<crate::kernel::directive::DirectiveService>>,
        memory_enabled: bool,
        memory_recall_budget: usize,
        agent_id: String,
        user_id: String,
        session_id: String,
    ) -> Self {
        Self {
            storage,
            org,
            tools,
            variables,
            prompt_config,
            cwd,
            cluster_client,
            directive,
            memory_enabled,
            memory_recall_budget,
            agent_id,
            user_id,
            session_id,
        }
    }
}

#[async_trait]
impl PromptResolver for CloudPromptResolver {
    async fn resolve(&self, meta: &PromptRequestMeta) -> Result<String> {
        let directive_prompt = self.directive.as_ref().and_then(|d| d.cached_prompt());

        let mut pb = CloudPromptLoader::new(self.storage.clone(), self.org.skills().clone())
            .with_tools(self.tools.clone())
            .with_variables(self.variables.clone())
            .with_cached_config(self.prompt_config.clone())
            .with_cwd(self.cwd.clone());

        if let Some(ref cc) = self.cluster_client {
            pb = pb.with_cluster_client(cc.clone());
        }
        pb = pb.with_directive_prompt(directive_prompt);

        let recall_memory = self.org.memory().filter(|_| self.memory_enabled).cloned();
        pb = pb.with_memory_service(recall_memory, self.memory_recall_budget);
        pb = pb.with_overlays(meta.system_overlay.clone(), meta.skill_overlay.clone());

        pb.build(&self.agent_id, &self.user_id, &self.session_id)
            .await
    }
}
