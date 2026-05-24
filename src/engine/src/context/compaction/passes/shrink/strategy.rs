//! Strategy classification for shrink decisions.

use std::collections::HashMap;

use crate::context::compaction::policy::ToolPolicy;
use crate::context::tokens::content_tokens;
use crate::types::*;

pub enum ToolShrinkDecision {
    Keep,
    OversizeCap,
    AgeClear,
    NormalTrunc,
}

pub struct ToolDecisionInput<'a> {
    pub tokens: usize,
    pub is_recent: bool,
    pub running_tokens: usize,
    pub budget: usize,
    pub oversize_token_threshold: usize,
    pub policy: &'a ToolPolicy,
}

pub fn classify_tool_result(input: ToolDecisionInput<'_>) -> ToolShrinkDecision {
    // Single oversized results are capped even inside the recent window.
    if input.tokens >= input.oversize_token_threshold {
        return ToolShrinkDecision::OversizeCap;
    }

    if input.is_recent {
        return ToolShrinkDecision::Keep;
    }

    // Age-based clearing for tools that support it
    if let Some(threshold) = input.policy.age_clear_threshold {
        if input.tokens > threshold && input.running_tokens > input.budget {
            return ToolShrinkDecision::AgeClear;
        }
    }

    // Normal truncation when over budget
    if input.running_tokens > input.budget {
        return ToolShrinkDecision::NormalTrunc;
    }

    ToolShrinkDecision::Keep
}

pub enum UserAction {
    Keep,
    TruncateOversized,
    StripImages,
}

pub fn classify_user_action(
    is_pinned: bool,
    is_recent: bool,
    _running_tokens: usize,
    _budget: usize,
    image_pressure: bool,
    content: &[Content],
    oversize_threshold: usize,
) -> UserAction {
    let has_images = content.iter().any(|c| matches!(c, Content::Image { .. }));
    if image_pressure && has_images {
        return UserAction::StripImages;
    }

    if is_pinned || is_recent {
        return UserAction::Keep;
    }

    let tokens = content_tokens(content);
    if tokens > oversize_threshold {
        return UserAction::TruncateOversized;
    }

    UserAction::Keep
}

/// Build an index from tool_call_id → tool_use input (for outline extraction).
/// Build an index from tool_call_id → tool_use arguments (serialized).
pub fn build_tool_call_index(messages: &[AgentMessage]) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for block in content {
                if let Content::ToolCall { id, arguments, .. } = block {
                    index.insert(id.clone(), arguments.to_string());
                }
            }
        }
    }
    index
}
