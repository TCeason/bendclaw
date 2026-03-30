use std::sync::Arc;

use parking_lot::RwLock;

use crate::kernel::agent_store::AgentStore;
use crate::kernel::directive::DirectiveService;
use crate::kernel::run::hooks::BeforeTurnHook;
use crate::kernel::run::hooks::SteeringSource;
use crate::kernel::run::prompt::PromptConfig;
use crate::kernel::run::prompt::PromptVariable;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::org::OrgServices;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::tools::registry::ToolRegistry;
use crate::llm::provider::LLMProvider;
use crate::llm::tool::ToolSchema;

pub struct SessionResources {
    pub workspace: Arc<Workspace>,
    pub tool_registry: Arc<ToolRegistry>,
    pub org: Arc<OrgServices>,
    pub tools: Arc<Vec<ToolSchema>>,
    pub storage: Arc<AgentStore>,
    pub llm: Arc<RwLock<Arc<dyn LLMProvider>>>,
    pub config: Arc<AgentConfig>,
    pub prompt_variables: Vec<PromptVariable>,
    pub cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
    pub directive: Option<Arc<DirectiveService>>,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    pub trace_writer: crate::kernel::trace::TraceWriter,
    pub persist_writer: crate::kernel::run::persist_op::PersistWriter,
    pub prompt_config: Option<PromptConfig>,
    pub before_turn_hook: Option<Arc<dyn BeforeTurnHook>>,
    pub steering_source: Option<Arc<dyn SteeringSource>>,
}
