use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;

use super::common;
use crate::base::Result;
use crate::kernel::run::prompt::resolver::LocalPromptResolver;
use crate::kernel::run::prompt::PromptSeed;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::assembly::contract::AgentContext;
use crate::kernel::session::assembly::contract::RunLabels;
use crate::kernel::session::assembly::contract::RuntimeInfra;
use crate::kernel::session::assembly::contract::SessionAssembly;
use crate::kernel::session::assembly::contract::SessionCore;
use crate::kernel::session::backend::noop::NoopBackend;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::services::NoopSecretUsageSink;

/// Assembles a minimal session for local (CLI one-shot) use.
/// Core tools only, no DB, no persistence.
pub struct LocalAssembler {
    pub runtime: Arc<Runtime>,
}

impl LocalAssembler {
    pub async fn assemble(
        &self,
        session_id: &str,
        opts: LocalBuildOptions,
    ) -> Result<SessionAssembly> {
        let llm = opts.llm_override.unwrap_or_else(|| self.runtime.llm());

        let workspace = common::build_workspace_ephemeral(
            &self.runtime.config,
            session_id,
            opts.cwd.as_deref(),
        )?;

        // Core tools only — no persistent or optional tools
        let secret_sink: Arc<dyn crate::kernel::tools::services::SecretUsageSink> =
            Arc::new(NoopSecretUsageSink);
        let mut registry = ToolRegistry::new();
        crate::kernel::tools::catalog::core::register(&mut registry, secret_sink);
        let registry = Arc::new(registry);

        let mut tools = registry.tool_schemas();
        let allowed_tool_names = common::apply_tool_filter(&mut tools, opts.tool_filter);

        let tools_arc = Arc::new(tools);

        let prompt_resolver = Arc::new(LocalPromptResolver::new(
            PromptSeed::default(),
            tools_arc.clone(),
            workspace.cwd().to_path_buf(),
        ));

        let noop = Arc::new(NoopBackend);

        Ok(SessionAssembly {
            labels: RunLabels {
                agent_id: "agent".into(),
                user_id: "cli".into(),
                session_id: session_id.into(),
            },
            core: SessionCore {
                workspace,
                llm: Arc::new(RwLock::new(llm.clone())),
                tool_registry: registry,
                tools: tools_arc,
                allowed_tool_names,
                prompt_resolver,
                context_provider: noop.clone(),
                run_initializer: noop,
            },
            infra: RuntimeInfra {
                storage: Arc::new(crate::kernel::agent_store::AgentStore::new(
                    crate::storage::pool::Pool::noop(),
                    llm.clone(),
                )),
                tool_writer: self.runtime.tool_writer.clone(),
                trace_writer: self.runtime.trace_writer.clone(),
                persist_writer: self.runtime.persist_writer.clone(),
            },
            agent: AgentContext {
                org: self.runtime.org.clone(),
                config: Arc::new(self.runtime.config.clone()),
                cluster_client: None,
                directive: None,
                prompt_config: None,
                prompt_variables: vec![],
                skill_executor: None,
                memory_recaller: None,
            },
        })
    }
}

/// Build options for ephemeral sessions.
#[derive(Default)]
pub struct LocalBuildOptions {
    pub cwd: Option<std::path::PathBuf>,
    pub tool_filter: Option<HashSet<String>>,
    pub llm_override: Option<Arc<dyn crate::llm::provider::LLMProvider>>,
}
