use crate::context::tokens::total_tokens;
use crate::context::tracking::CompactionBudgetState;
use crate::context::tracking::ContextConfig;
use crate::types::*;

#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    pub message_tokens: usize,
    pub estimated_tokens: usize,
    pub budget: usize,
    pub compact_trigger: usize,
    pub compact_target: usize,
    pub message_count: usize,
    pub image_count: usize,
    pub max_messages: usize,
}

impl ContextSnapshot {
    pub fn new(
        messages: &[AgentMessage],
        config: &ContextConfig,
        budget_state: &CompactionBudgetState,
    ) -> Self {
        let budget = config
            .max_context_tokens
            .saturating_sub(config.system_prompt_tokens);
        let compact_trigger = budget * (config.compact_trigger_pct.min(100) as usize) / 100;
        let compact_target_pct = config.compact_target_pct.min(config.compact_trigger_pct);
        let compact_target = budget * (compact_target_pct as usize) / 100;

        Self {
            message_tokens: total_tokens(messages),
            estimated_tokens: budget_state.estimated_tokens,
            budget,
            compact_trigger,
            compact_target,
            message_count: messages.len(),
            image_count: image_count(messages),
            max_messages: config.max_messages,
        }
    }

    pub fn effective_tokens(&self, system_prompt_tokens: usize) -> usize {
        if self.image_count > 0 {
            self.message_tokens
                .max(self.estimated_tokens.saturating_sub(system_prompt_tokens))
        } else {
            self.message_tokens
        }
    }

    pub fn over_message_limit(&self) -> bool {
        self.max_messages > 0 && self.message_count > self.max_messages
    }
}

fn image_count(messages: &[AgentMessage]) -> usize {
    messages
        .iter()
        .map(|message| match message {
            AgentMessage::Llm(Message::User { content, .. })
            | AgentMessage::Llm(Message::Assistant { content, .. })
            | AgentMessage::Llm(Message::ToolResult { content, .. }) => content
                .iter()
                .filter(|content| matches!(content, Content::Image { .. }))
                .count(),
            AgentMessage::Extension(_) => 0,
        })
        .sum()
}
