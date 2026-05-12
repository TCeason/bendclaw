use crate::context::compaction::policy::ToolPolicy;

pub(super) enum ToolShrinkDecision {
    Keep,
    OversizeCap,
    AgeClear,
    NormalTrunc,
}

pub(super) struct ToolDecisionInput<'a> {
    pub tokens: usize,
    pub is_recent: bool,
    pub running_tokens: usize,
    pub budget: usize,
    pub oversize_token_threshold: usize,
    pub policy: &'a ToolPolicy,
}

pub(super) fn classify_tool_result(input: ToolDecisionInput<'_>) -> ToolShrinkDecision {
    if input.tokens > input.oversize_token_threshold {
        return ToolShrinkDecision::OversizeCap;
    }

    let over_budget = input.running_tokens > input.budget;
    if over_budget && !input.is_recent {
        if let Some(threshold) = input.policy.age_clear_threshold {
            if input.tokens > threshold {
                return ToolShrinkDecision::AgeClear;
            }
        }
    }

    if over_budget {
        return ToolShrinkDecision::NormalTrunc;
    }

    ToolShrinkDecision::Keep
}
