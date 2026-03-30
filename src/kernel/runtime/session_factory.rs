use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::prompt::PromptConfig;
use crate::kernel::run::prompt::PromptVariable;
use crate::kernel::runtime::diagnostics;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::workspace::OpenResolver;
use crate::kernel::session::workspace::SandboxResolver;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::session::Session;
use crate::kernel::session::SessionResources;
use crate::kernel::tools::registry::create_session_tools;
use crate::kernel::tools::registry::register_cluster_tools;
use crate::kernel::tools::registry::register_memory_tools;

impl Runtime {
    pub async fn get_or_create_session(
        self: &Arc<Self>,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
    ) -> Result<Arc<Session>> {
        self.require_ready()?;

        if let Some(session) = self.sessions.get(session_id) {
            if !session.belongs_to(agent_id, user_id) {
                diagnostics::log_runtime_denied(agent_id, user_id, session_id);
                return Err(ErrorCode::denied(format!(
                    "session '{session_id}' belongs to a different agent/user"
                )));
            }
            if session.is_stale() && !session.is_running() {
                self.sessions.remove(session_id);
                diagnostics::log_runtime_recreated(agent_id, user_id, session_id);
            } else {
                diagnostics::log_runtime_reused(agent_id, user_id, session_id);
                return Ok(session);
            }
        }

        let pool = self.databases.agent_pool(agent_id)?;

        let workspace_dir = self
            .config
            .workspace
            .session_dir(user_id, agent_id, session_id);
        if let Err(e) = std::fs::create_dir_all(&workspace_dir) {
            return Err(ErrorCode::internal(format!(
                "failed to create session workspace: {e}"
            )));
        }

        // Parallelize the two independent DB queries: agent config + variables.
        let (llm_config_result, vars_result) = tokio::join!(
            self.resolve_agent_llm_and_config(agent_id, &pool),
            self.org.variables().list_active(user_id)
        );

        let (agent_llm, cached_config) = llm_config_result?;
        let variables = vars_result
            .map_err(|e| ErrorCode::internal(format!("failed to load variables: {e}")))?;
        // Deduplicate by key: owned variables take precedence over subscribed ones.
        // list_active returns owned first (user_id = ?) then subscribed via UNION.
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

        let storage = Arc::new(AgentStore::new(pool.clone(), agent_llm.clone()));

        let resolver: Arc<dyn crate::kernel::session::workspace::PathResolver> =
            if self.config.workspace.sandbox {
                Arc::new(SandboxResolver)
            } else {
                Arc::new(OpenResolver)
            };

        // cwd: where shell/grep/glob operate by default.
        // sandbox=true  → workspace dir (agent is isolated)
        // sandbox=false → $HOME (agent can navigate the user's filesystem)
        let cwd = if self.config.workspace.sandbox {
            workspace_dir.clone()
        } else {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| workspace_dir.clone())
        };

        let workspace = Arc::new(Workspace::from_variables(
            workspace_dir,
            cwd,
            self.config.workspace.safe_env_vars.clone(),
            &variables,
            Duration::from_secs(self.config.workspace.command_timeout_secs),
            Duration::from_secs(self.config.workspace.max_command_timeout_secs),
            self.config.workspace.max_output_bytes,
            resolver,
        ));

        let mut tool_registry = create_session_tools(
            self.org.clone(),
            pool.clone(),
            self.channels.clone(),
            self.config.node_id.clone(),
        );

        // Conditionally register cluster tools
        if let Some(ref svc) = *self.cluster.read() {
            let dt = svc.create_dispatch_table();
            register_cluster_tools(&mut tool_registry, svc.clone(), dt);
        }

        // Conditionally register memory tools
        if let Some(mem) = self.org.memory() {
            register_memory_tools(&mut tool_registry, mem.clone());
        }

        let tool_registry = Arc::new(tool_registry);

        let mut tools = tool_registry.tool_schemas();
        let existing_names: std::collections::HashSet<String> =
            tools.iter().map(|t| t.function.name.clone()).collect();
        for skill in self.org.skills().list(user_id) {
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

        let tool_count = tools.len();

        let session = Arc::new(Session::new(
            session_id.to_string(),
            agent_id.into(),
            user_id.into(),
            SessionResources {
                workspace,
                tool_registry,
                org: self.org.clone(),
                tools: Arc::new(tools),
                storage,
                llm: Arc::new(RwLock::new(agent_llm)),
                config: Arc::new(self.config.clone()),
                prompt_variables,
                cluster_client: self.cluster.read().clone(),
                directive: self.directive.read().clone(),
                trace_writer: self.trace_writer.clone(),
                persist_writer: self.persist_writer.clone(),
                tool_writer: self.tool_writer.clone(),
                prompt_config,
                before_turn_hook: None,
                steering_source: None,
            },
        ));

        self.sessions.insert(session.clone());

        diagnostics::log_runtime_session_created(
            agent_id,
            user_id,
            session_id,
            &self
                .config
                .workspace
                .session_dir(user_id, agent_id, session_id)
                .display()
                .to_string(),
            tool_count,
        );

        Ok(session)
    }
}
