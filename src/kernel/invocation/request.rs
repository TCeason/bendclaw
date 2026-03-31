//! Invocation request types — orthogonal dimensions for a run.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::kernel::channel::context::ChannelContext;
use crate::kernel::session::options::RunOptions;

/// Where config/tools/skills come from.
pub enum ConfigSource {
    Local,
    Cloud { agent_id: String, user_id: String },
}

/// Whether the session is persisted.
pub enum PersistenceMode {
    /// No DB persistence — transient session, discarded after run.
    Noop,
    /// DB-backed session with cache/reuse/stale policy. Caller owns session_id.
    Persistent { session_id: String },
}

/// Per-invocation conversation context.
pub enum ConversationContext {
    None,
    Channel(ChannelContext),
}

/// A complete invocation request.
pub struct InvocationRequest {
    pub source: ConfigSource,
    pub persistence: PersistenceMode,
    pub context: ConversationContext,
    pub prompt: String,
    pub options: RunOptions,
    pub session_options: SessionBuildOptions,
}

/// Build-time options for the session (workspace, tool filter, LLM override).
#[derive(Default)]
pub struct SessionBuildOptions {
    pub cwd: Option<PathBuf>,
    pub tool_filter: Option<HashSet<String>>,
    pub llm_override: Option<Arc<dyn crate::llm::provider::LLMProvider>>,
}
