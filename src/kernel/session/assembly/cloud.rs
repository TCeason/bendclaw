use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;

use super::common;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::prompt::resolver::CloudPromptResolver;
use crate::kernel::run::prompt::PromptConfig;
use crate::kernel::run::prompt::PromptVariable;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::assembly::contract::AgentContext;
use crate::kernel::session::assembly::contract::RunLabels;
use crate::kernel::session::assembly::contract::RuntimeInfra;
use crate::kernel::session::assembly::contract::SessionAssembly;
use crate::kernel::session::assembly::contract::SessionCore;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::services::DbSecretUsageSink;

/// Assembles a full session with cloud config, all tools, skills, memory.
pub struct CloudAssembler {
    pub runtime: Arc<Runtime>,
}

impl CloudAssembler {
    pub async fn assemble(
        &self,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
        opts: CloudBuildOptions,
    ) -> Result<SessionAssembly> {
        let pool = self.runtime.databases.agent_pool(agent_id)?;

        // LLM + config
        let (agent_llm, cached_config) = match opts.llm_override {
            Some(llm) => (llm, None),
            None => {
                self.runtime
                    .resolve_agent_llm_and_config(agent_id, &pool)
                    .await?
            }
        };

        // Variables
        let variables = self
            .runtime
            .org
            .variables()
            .list_active(user_id)
            .await
            .map_err(|e| {
                crate::base::ErrorCode::internal(format!("failed to load variables: {e}"))
            })?;
        let variables: Vec<_> = {
            let mut seen = std::collections::HashSet::new();
            variables
                .into_iter()
                .filter(|v| seen.insert(v.key.clone()))
                .collect()
        };
        let prompt_variables: Vec<PromptVariable> =
            variables.iter().map(PromptVariable::from).collect();
        let prompt_config = cached_config.clone().map(PromptConfig::from);

        // Workspace
        let workspace = common::build_workspace(
            &self.runtime.config,
            agent_id,
            session_id,
            user_id,
            opts.cwd.as_deref(),
            &variables,
        )?;

        // Storage
        let storage = Arc::new(AgentStore::new(pool.clone(), agent_llm.clone()));

        // Tools: core + persistent + optional
        let secret_sink: Arc<dyn crate::kernel::tools::services::SecretUsageSink> =
            Arc::new(DbSecretUsageSink::new(pool.clone()));
        let mut registry = ToolRegistry::new();
        crate::kernel::tools::catalog::core::register(&mut registry, secret_sink);
        crate::kernel::tools::catalog::cloud::register(
            &mut registry,
            self.runtime.org.clone(),
            pool.clone(),
            self.runtime.channels.clone(),
            self.runtime.config.node_id.clone(),
        );
        let cluster_ref = self.runtime.cluster.read().clone();
        let memory_ref = self.runtime.org.memory().cloned();
        if let Some(ref svc) = cluster_ref {
            let dt = svc.create_dispatch_table();
            crate::kernel::tools::catalog::optional::register(
                &mut registry,
                Some((svc, &dt)),
                memory_ref.as_ref(),
            );
        } else {
            crate::kernel::tools::catalog::optional::register(
                &mut registry,
                None,
                memory_ref.as_ref(),
            );
        }
        let registry = Arc::new(registry);

        // Tool schemas + executable skill tools
        let mut tools = registry.tool_schemas();
        let existing_names: HashSet<String> =
            tools.iter().map(|t| t.function.name.clone()).collect();
        for skill in self.runtime.org.skills().list(user_id) {
            if !skill.executable {
                continue;
            }
            let tool_name = crate::kernel::skills::tool_key::format(&skill, user_id);
            if existing_names.contains(&tool_name) {
                continue;
            }
            let params = skill.to_json_schema();
            tools.push(crate::llm::tool::ToolSchema::new(
                &tool_name,
                &skill.description,
                params,
            ));
        }
        let allowed_tool_names = common::apply_tool_filter(&mut tools, opts.tool_filter);

        let tools_arc = Arc::new(tools);

        let prompt_resolver = Arc::new(CloudPromptResolver::new(
            storage.clone(),
            self.runtime.org.clone(),
            tools_arc.clone(),
            prompt_variables.clone(),
            prompt_config.clone(),
            workspace.cwd().to_path_buf(),
            cluster_ref.clone(),
            self.runtime.directive.read().clone(),
            self.runtime.config.memory.recall,
            self.runtime.config.memory.recall_budget,
            agent_id.to_string(),
            user_id.to_string(),
            session_id.to_string(),
        ));

        // Backend: use existing storage/writers for now (full PersistentBackend comes later)
        let noop = Arc::new(crate::kernel::session::backend::noop::NoopBackend);

        Ok(SessionAssembly {
            labels: RunLabels {
                agent_id: agent_id.into(),
                user_id: user_id.into(),
                session_id: session_id.into(),
            },
            core: SessionCore {
                workspace,
                llm: Arc::new(RwLock::new(agent_llm)),
                tool_registry: registry,
                tools: tools_arc,
                allowed_tool_names,
                prompt_resolver,
                context_provider: noop.clone(),
                run_initializer: noop,
            },
            infra: RuntimeInfra {
                storage,
                tool_writer: self.runtime.tool_writer.clone(),
                trace_writer: self.runtime.trace_writer.clone(),
                persist_writer: self.runtime.persist_writer.clone(),
            },
            agent: AgentContext {
                org: self.runtime.org.clone(),
                config: Arc::new(self.runtime.config.clone()),
                cluster_client: cluster_ref,
                directive: self.runtime.directive.read().clone(),
                prompt_config,
                prompt_variables,
                skill_executor: None,
                memory_recaller: None,
            },
        })
    }
}

/// Build options for persistent sessions.
#[derive(Default)]
pub struct CloudBuildOptions {
    pub cwd: Option<std::path::PathBuf>,
    pub tool_filter: Option<HashSet<String>>,
    pub llm_override: Option<Arc<dyn crate::llm::provider::LLMProvider>>,
}
