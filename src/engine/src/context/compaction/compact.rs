use super::config::CompactionConfig;
pub use super::passes::shrink::truncate::truncate_text_head_tail;
use super::pipeline;
pub use super::types::CompactionAction;
pub use super::types::CompactionMethod;
pub use super::types::CompactionResult;
pub use super::types::CompactionStats;
pub use super::types::ToolTokenDetail;
use crate::context::tracking::CompactionBudgetState;
use crate::context::tracking::ContextConfig;
use crate::types::AgentMessage;

pub fn compact_messages(
    messages: Vec<AgentMessage>,
    config: &ContextConfig,
    budget_state: &CompactionBudgetState,
) -> CompactionResult {
    let compaction_config = CompactionConfig::from_context_config(config);
    pipeline::run(messages, &compaction_config, budget_state.estimated_tokens)
}
