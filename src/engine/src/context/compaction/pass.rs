//! CompactionPass trait and shared context.

use super::compact::CompactionAction;
use super::policy::CompactionPolicy;
use crate::types::AgentMessage;

/// Shared read-only context passed to every pass.
pub struct CompactionContext {
    pub budget: usize,
    pub keep_recent: usize,
    pub keep_first: usize,
    pub tool_output_max_lines: usize,
    pub policy: CompactionPolicy,
}

/// Output of a single pass.
pub struct PassResult {
    pub messages: Vec<AgentMessage>,
    pub actions: Vec<CompactionAction>,
}

/// A single compaction pass.
///
/// Each pass receives the full message list and context, and returns
/// a (possibly modified) message list plus a record of what it did.
#[allow(dead_code)]
pub trait CompactionPass: Send + Sync {
    fn name(&self) -> &str;
    fn run(&self, messages: Vec<AgentMessage>, ctx: &CompactionContext) -> PassResult;
}
