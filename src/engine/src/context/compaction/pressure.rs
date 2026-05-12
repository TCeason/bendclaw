use super::snapshot::ContextSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictMode {
    MessageLimit,
    TokenBudget,
}

#[derive(Debug, Clone, Copy)]
pub struct PressureState {
    pub effective_tokens: usize,
    pub over_budget_tokens: usize,
    pub over_message_tokens: usize,
    pub over_message_limit: bool,
}

impl PressureState {
    pub fn classify(snapshot: &ContextSnapshot, system_prompt_tokens: usize) -> Self {
        let effective_tokens = snapshot.effective_tokens(system_prompt_tokens);
        let over_budget_tokens = effective_tokens.saturating_sub(snapshot.budget);
        let over_message_tokens = snapshot.message_tokens.saturating_sub(snapshot.budget);
        let over_message_limit = snapshot.over_message_limit();

        Self {
            effective_tokens,
            over_budget_tokens,
            over_message_tokens,
            over_message_limit,
        }
    }

    pub fn needs_collapse(self, snapshot: &ContextSnapshot) -> bool {
        self.effective_tokens > snapshot.compact_trigger && !self.over_message_limit
    }

    pub fn evict_mode(self) -> Option<EvictMode> {
        if self.over_message_tokens > 0 {
            return Some(EvictMode::TokenBudget);
        }
        if self.over_message_limit {
            return Some(EvictMode::MessageLimit);
        }
        if self.over_budget_tokens > 0 {
            return Some(EvictMode::TokenBudget);
        }
        None
    }

    pub fn needs_evict(self) -> bool {
        self.evict_mode().is_some()
    }
}
