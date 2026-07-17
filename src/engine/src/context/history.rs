//! Provider-aware normalization of prior assistant content before an LLM call.

use crate::provider::ApiProtocol;
use crate::types::Content;
use crate::types::Message;

/// Preserve replayable thinking only for the exact provider/model/protocol that
/// produced it. Foreign or unsigned thinking remains useful context, but is
/// downgraded to ordinary text so provider-specific opaque state is never sent
/// to the wrong API.
pub fn transform_messages_for_model(
    messages: Vec<Message>,
    target_provider: &str,
    target_model: &str,
    target_api: ApiProtocol,
) -> Vec<Message> {
    messages
        .into_iter()
        .map(|message| transform_message(message, target_provider, target_model, target_api))
        .collect()
}

fn transform_message(
    message: Message,
    target_provider: &str,
    target_model: &str,
    target_api: ApiProtocol,
) -> Message {
    let Message::Assistant {
        content,
        stop_reason,
        model,
        provider,
        usage,
        timestamp,
        error_message,
        response_id,
    } = message
    else {
        return message;
    };

    let same_model = provider == target_provider && model == target_model;
    let content = content
        .into_iter()
        .filter_map(|block| match block {
            Content::Thinking { thinking, metadata }
                if same_model
                    && metadata
                        .as_ref()
                        .is_some_and(|value| value.supports_api(target_api)) =>
            {
                Some(Content::Thinking { thinking, metadata })
            }
            Content::Thinking { thinking, .. } if thinking.trim().is_empty() => None,
            Content::Thinking { thinking, .. } => Some(Content::Text { text: thinking }),
            Content::ToolCall {
                id,
                name,
                arguments,
            } if target_api == ApiProtocol::OpenAiResponses && !same_model => {
                Some(Content::ToolCall {
                    id: id
                        .split_once('|')
                        .map(|(call_id, _)| call_id)
                        .unwrap_or(&id)
                        .to_string(),
                    name,
                    arguments,
                })
            }
            other => Some(other),
        })
        .collect();

    Message::Assistant {
        content,
        stop_reason,
        model,
        provider,
        usage,
        timestamp,
        error_message,
        response_id,
    }
}
