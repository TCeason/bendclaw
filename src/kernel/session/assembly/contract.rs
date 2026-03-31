use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::base::Result;
use crate::kernel::directive::DirectiveService;
use crate::kernel::run::prompt::PromptConfig;
use crate::kernel::run::prompt::PromptResolver;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::org::OrgServices;
use crate::kernel::session::backend::context::SessionContextProvider;
use crate::kernel::session::backend::sink::RunInitializer;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::tools::registry::ToolRegistry;
use crate::llm::provider::LLMProvider;
use crate::llm::tool::ToolSchema;

/// Labels for logging, tracing, and run records. Not an identity model.
#[derive(Debug, Clone)]
pub struct RunLabels {
    pub agent_id: Arc<str>,
    pub user_id: Arc<str>,
    pub session_id: Arc<str>,
}

/// The single product of both assemblers. Session doesn't care how it was built.
pub struct SessionAssembly {
    pub labels: RunLabels,
    pub core: SessionCore,
    pub infra: RuntimeInfra,
    pub agent: AgentContext,
}

/// Session-essential dependencies: workspace, LLM, tools, prompt, backend.
pub struct SessionCore {
    pub workspace: Arc<Workspace>,
    pub llm: Arc<RwLock<Arc<dyn LLMProvider>>>,
    pub tool_registry: Arc<ToolRegistry>,
    pub tools: Arc<Vec<ToolSchema>>,
    pub allowed_tool_names: Option<HashSet<String>>,
    pub prompt_resolver: Arc<dyn PromptResolver>,
    pub context_provider: Arc<dyn SessionContextProvider>,
    pub run_initializer: Arc<dyn RunInitializer>,
}

/// Infrastructure: storage, writers, tracing.
pub struct RuntimeInfra {
    pub storage: Arc<crate::kernel::agent_store::AgentStore>,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    pub trace_writer: crate::kernel::trace::TraceWriter,
    pub persist_writer: crate::kernel::run::persist_op::PersistWriter,
}

/// Agent-level context: org, config, prompt data, optional services.
pub struct AgentContext {
    pub org: Arc<OrgServices>,
    pub config: Arc<AgentConfig>,
    pub cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
    pub directive: Option<Arc<DirectiveService>>,
    pub prompt_config: Option<PromptConfig>,
    pub prompt_variables: Vec<crate::kernel::run::prompt::PromptVariable>,
    pub skill_executor: Option<Arc<dyn SkillExecutor>>,
    pub memory_recaller: Option<Arc<dyn MemoryRecaller>>,
}

/// Executable skill executor. Persistent sessions have one; ephemeral don't.
#[async_trait]
pub trait SkillExecutor: Send + Sync {
    async fn execute(&self, skill_name: &str, input: &str) -> Result<String>;
}

/// Runtime memory recall. Persistent sessions have one; ephemeral don't.
#[async_trait]
pub trait MemoryRecaller: Send + Sync {
    async fn recall(&self, query: &str) -> Result<Option<String>>;
}
