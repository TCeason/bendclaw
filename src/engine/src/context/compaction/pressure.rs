use super::config::CompactionConfig;
use super::policy::tool_policy::is_compactable_tool_result;
use crate::context::tokens::content_tokens;
use crate::context::tokens::total_tokens;
use crate::types::*;

#[derive(Clone, Copy, Debug)]
pub struct Pressure {
    pub message_tokens: usize,
    pub estimated_tokens: usize,
    pub compactable_tool_result_tokens: usize,
    #[allow(dead_code)]
    pub max_tool_result_tokens: usize,
    #[allow(dead_code)]
    pub max_user_tokens: usize,
    pub message_count: usize,
    pub image_pressure: bool,
}

impl Pressure {
    pub fn from_messages(
        messages: &[AgentMessage],
        config: &CompactionConfig,
        estimated_tokens: usize,
    ) -> Self {
        let message_tokens = total_tokens(messages);
        let (compactable_tool_result_tokens, max_tool_result_tokens, max_user_tokens, image_tokens) =
            compute_message_stats(messages);
        let image_pressure = image_tokens > config.budget_tokens / 4
            && (estimated_tokens > config.compact_trigger()
                || image_tokens > config.budget_tokens / 2);

        Self {
            message_tokens,
            estimated_tokens,
            compactable_tool_result_tokens,
            max_tool_result_tokens,
            max_user_tokens,
            message_count: messages.len(),
            image_pressure,
        }
    }
}

fn compute_message_stats(messages: &[AgentMessage]) -> (usize, usize, usize, usize) {
    let mut compactable_tokens = 0usize;
    let mut max_tool_tokens = 0usize;
    let mut max_user_tokens = 0usize;
    let mut image_tokens = 0usize;

    for msg in messages {
        match msg {
            AgentMessage::Llm(Message::ToolResult {
                tool_name, content, ..
            }) => {
                let tokens = content_tokens(content);
                max_tool_tokens = max_tool_tokens.max(tokens);
                let text_len: usize = content
                    .iter()
                    .map(|c| match c {
                        Content::Text { text } => text.len(),
                        _ => 0,
                    })
                    .sum();
                if is_compactable_tool_result(tool_name, text_len) {
                    compactable_tokens += tokens;
                }
            }
            AgentMessage::Llm(Message::User { content, .. }) => {
                let user_tokens = content_tokens(content);
                max_user_tokens = max_user_tokens.max(user_tokens);
                for c in content {
                    if matches!(c, Content::Image { .. }) {
                        image_tokens += content_tokens(std::slice::from_ref(c));
                    }
                }
            }
            _ => {}
        }
    }

    (
        compactable_tokens,
        max_tool_tokens,
        max_user_tokens,
        image_tokens,
    )
}
