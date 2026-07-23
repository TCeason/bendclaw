//! LLM-based summarization — calls the provider to generate structured summaries.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::mode::SummarizerContext;
use super::prompt;
use super::types::SummarizerError;
use super::types::SummarizerInput;
use super::types::SummarizerOutput;
use crate::context::compaction::types::FileOps;
use crate::provider::StreamConfig;
use crate::provider::StreamEvent;
use crate::types::*;

/// Generate a summary using the LLM provider.
pub async fn summarize(
    input: SummarizerInput,
    ctx: &SummarizerContext,
    reserve_tokens: u32,
    cancel: CancellationToken,
) -> Result<SummarizerOutput, SummarizerError> {
    let main_max_tokens = output_budget(ctx, reserve_tokens.saturating_mul(4) / 5);
    let prefix_max_tokens = output_budget(ctx, reserve_tokens / 2);

    // A split can begin at the current turn, leaving no prior history. Match
    // pi by avoiding a useless first model request in that case.
    let mut summary = if input.conversation.trim().is_empty() && input.turn_prefix.is_some() {
        "No prior history.".to_string()
    } else {
        let user_prompt = match &input.previous_summary {
            Some(previous) => prompt::format_update(
                &input.conversation,
                previous,
                input.custom_instructions.as_deref(),
            ),
            None => {
                prompt::format_initial(&input.conversation, input.custom_instructions.as_deref())
            }
        };
        call_provider(ctx, &user_prompt, main_max_tokens, cancel.clone()).await?
    };

    if let Some(prefix_text) = &input.turn_prefix {
        let prefix_prompt = prompt::format_turn_prefix(prefix_text);
        let prefix_summary = call_provider(ctx, &prefix_prompt, prefix_max_tokens, cancel).await?;
        summary.push_str("\n\n---\n\n**Turn Context (split turn):**\n\n");
        summary.push_str(&prefix_summary);
    }

    // Append file operations (rule-based, not LLM-generated)
    summary.push_str(&format_file_ops_xml(&input.file_ops));

    Ok(SummarizerOutput { summary })
}

fn output_budget(ctx: &SummarizerContext, requested: u32) -> u32 {
    let model_max = ctx
        .model_config
        .as_ref()
        .map(|model| model.max_tokens)
        .unwrap_or(u32::MAX);
    requested.min(model_max).max(1)
}

/// Call the provider for a single summarization request.
async fn call_provider(
    ctx: &SummarizerContext,
    user_prompt: &str,
    max_tokens: u32,
    cancel: CancellationToken,
) -> Result<String, SummarizerError> {
    let messages = vec![Message::User {
        content: vec![Content::Text {
            text: user_prompt.to_string(),
        }],
        timestamp: now_ms(),
    }];

    let config = StreamConfig {
        model: ctx.model.clone(),
        system_prompt: prompt::SYSTEM_PROMPT.to_string(),
        messages,
        tools: vec![],
        thinking_level: ctx.thinking_level,
        api_key: ctx.api_key.clone(),
        max_tokens: Some(max_tokens),
        model_config: ctx.model_config.clone(),
        cache_config: CacheConfig::default(),
        prompt_cache_key: None,
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();

    let result = ctx
        .provider
        .stream(config, tx, cancel.clone())
        .await
        .map_err(|e| SummarizerError::Failed(e.to_string()))?;

    // Drain the channel (we don't need streaming events for summarization)
    while rx.recv().await.is_some() {}

    if cancel.is_cancelled() {
        return Err(SummarizerError::Cancelled);
    }

    let message = result.into_message();
    match message {
        Message::Assistant {
            content,
            stop_reason,
            error_message,
            ..
        } => {
            if stop_reason == StopReason::Error {
                return Err(SummarizerError::Failed(
                    error_message.unwrap_or_else(|| "Unknown error".into()),
                ));
            }
            let text = content
                .iter()
                .filter_map(|c| match c {
                    Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text)
        }
        _ => Err(SummarizerError::Failed("Unexpected response type".into())),
    }
}

/// Format file operations as XML tags (appended to LLM summary).
fn format_file_ops_xml(file_ops: &FileOps) -> String {
    let mut sections: Vec<String> = Vec::new();

    let read_only = file_ops.read_only();
    if !read_only.is_empty() {
        let files: Vec<&str> = read_only.iter().map(|s| s.as_str()).collect();
        sections.push(format!("<read-files>\n{}\n</read-files>", files.join("\n")));
    }

    let modified = file_ops.modified();
    if !modified.is_empty() {
        let files: Vec<&str> = modified.iter().map(|s| s.as_str()).collect();
        sections.push(format!(
            "<modified-files>\n{}\n</modified-files>",
            files.join("\n")
        ));
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", sections.join("\n\n"))
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
