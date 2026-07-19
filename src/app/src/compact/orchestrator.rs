use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::context_view::compact_summary_item;
use crate::agent::run::convert;
use crate::agent::session::Session;
use crate::conf::LlmConfig;
use crate::error::Result;
use crate::types::CompactDetails;
use crate::types::CompactReason;
use crate::types::CompactSplitTurn;
use crate::types::TranscriptItem;

#[derive(Debug, Clone)]
pub struct CompactSettings {
    pub keep_recent_tokens: usize,
    pub keep_recent_min_messages: usize,
    /// The active model's context window in tokens. Drives the shrink/reclaim
    /// transforms applied to the retained tail and the post-compaction fit
    /// check. `0` disables both (window unknown).
    pub context_window: usize,
}

impl Default for CompactSettings {
    fn default() -> Self {
        Self {
            keep_recent_tokens: 20_000,
            keep_recent_min_messages: 6,
            context_window: 0,
        }
    }
}

#[derive(Clone)]
pub struct ManualCompactRequest {
    pub reason: CompactReason,
    pub custom_instructions: Option<String>,
    pub summary_override: Option<String>,
    pub summarizer: Option<CompactSummarizer>,
    pub settings: CompactSettings,
}

#[derive(Clone)]
pub struct CompactSummarizer {
    pub provider: std::sync::Arc<dyn evot_engine::provider::StreamProvider>,
    pub llm: LlmConfig,
    pub max_tokens: u32,
    /// Maximum wall-clock time for one LLM summarization pass. Expiry uses the
    /// deterministic fallback; explicit user cancellation still cancels the
    /// entire compaction without writing a marker.
    pub timeout: std::time::Duration,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ManualCompactionOutcome {
    Compacted {
        summary: String,
        tokens_before: usize,
        tokens_after: usize,
        messages_before: usize,
        messages_after: usize,
        context_window: usize,
        used_fallback: bool,
    },
    NothingToCompact,
    Cancelled,
}

/// Result from the compaction orchestrator. `status` distinguishes cancellation
/// from an ordinary no-op so callers never report Esc as "Nothing to compact".
#[derive(Debug, PartialEq, Eq)]
pub enum CompactSessionStatus {
    Compacted,
    NothingToCompact,
    Cancelled,
}

#[derive(Debug)]
pub struct CompactSessionOutcome {
    pub status: CompactSessionStatus,
    pub item: Option<TranscriptItem>,
    pub used_fallback: bool,
}

pub async fn compact_session(
    session: &Arc<Session>,
    request: ManualCompactRequest,
    cancel: CancellationToken,
) -> Result<Option<TranscriptItem>> {
    Ok(compact_session_with_status(session, request, cancel)
        .await?
        .item)
}

pub async fn compact_session_with_status(
    session: &Arc<Session>,
    request: ManualCompactRequest,
    cancel: CancellationToken,
) -> Result<CompactSessionOutcome> {
    if cancel.is_cancelled() {
        return Ok(CompactSessionOutcome {
            status: CompactSessionStatus::Cancelled,
            item: None,
            used_fallback: false,
        });
    }

    let entries = session.load_all_entries().await?;
    let context_entries = super::context_view::resolve_context_entries(&entries);
    let compact_entries: Vec<evot_engine::CompactEntry> = context_entries
        .iter()
        .filter_map(|(seq, item)| {
            if *seq == 0 {
                return None;
            }
            Some(evot_engine::CompactEntry {
                seq: *seq,
                message: convert::agent_message_from_transcript(item),
            })
        })
        .collect();

    let previous = entries.iter().rev().find_map(|entry| match &entry.item {
        TranscriptItem::Compact {
            first_kept_seq,
            summary,
            details,
            ..
        } => Some((
            *first_kept_seq,
            evot_engine::truncate_summary(summary, evot_engine::DEFAULT_SUMMARY_MAX_BYTES),
            details.clone(),
        )),
        _ => None,
    });
    let boundary_seq = previous.as_ref().map(|(seq, _, _)| *seq);

    let plan = match evot_engine::plan_session_compaction(
        &compact_entries,
        boundary_seq,
        request.settings.keep_recent_tokens,
        request.settings.keep_recent_min_messages,
    ) {
        Some(plan) => plan,
        None => {
            return Ok(CompactSessionOutcome {
                status: CompactSessionStatus::NothingToCompact,
                item: None,
                used_fallback: false,
            })
        }
    };

    if cancel.is_cancelled() {
        return Ok(CompactSessionOutcome {
            status: CompactSessionStatus::Cancelled,
            item: None,
            used_fallback: false,
        });
    }

    let has_summarizer = request.summarizer.is_some();
    let summary_override = request.summary_override.filter(|s| !s.trim().is_empty());
    let mut used_fallback = !has_summarizer && summary_override.is_none();
    let summary = if let Some(summary) = summary_override {
        summary
    } else if let Some(summarizer) = request.summarizer.as_ref() {
        let generated = summarize_with_llm(
            &compact_entries,
            &plan,
            previous.as_ref().map(|(_, s, _)| s.as_str()),
            request.custom_instructions.as_deref(),
            summarizer,
            cancel.clone(),
        )
        .await;
        if cancel.is_cancelled() {
            return Ok(CompactSessionOutcome {
                status: CompactSessionStatus::Cancelled,
                item: None,
                used_fallback: false,
            });
        }
        match generated {
            Some(summary) => summary,
            None => {
                used_fallback = true;
                build_summary(
                    &compact_entries,
                    &plan,
                    previous.as_ref().map(|(_, s, _)| s.as_str()),
                    request.custom_instructions.as_deref(),
                )
            }
        }
    } else {
        build_summary(
            &compact_entries,
            &plan,
            previous.as_ref().map(|(_, s, _)| s.as_str()),
            request.custom_instructions.as_deref(),
        )
    };
    let summary = evot_engine::truncate_summary(&summary, evot_engine::DEFAULT_SUMMARY_MAX_BYTES);
    let new_context = build_new_context_items(&context_entries, plan.first_kept_seq, &summary);
    let messages_after = new_context.len();
    let tokens_after =
        evot_engine::context::total_tokens(&convert::into_agent_messages(&new_context));

    let mut details = CompactDetails::default();
    if let Some((_, _, prev_details)) = previous {
        details.read_files = prev_details.read_files;
        details.modified_files = prev_details.modified_files;
    }
    for file in plan.file_ops.read_only() {
        if !details.read_files.contains(file) {
            details.read_files.push(file.to_string());
        }
    }
    for file in plan.file_ops.modified() {
        if !details.modified_files.contains(file) {
            details.modified_files.push(file.to_string());
        }
    }
    details.read_files.sort();
    details.read_files.dedup();
    details.modified_files.sort();
    details.modified_files.dedup();

    let split_turn = plan.split_turn.map(|split| CompactSplitTurn {
        turn_start_seq: split.turn_start_seq,
        cut_seq: split.cut_seq,
    });

    let item = TranscriptItem::Compact {
        id: crate::types::new_id(),
        created_at: evot_engine::now_ms(),
        reason: request.reason,
        summary,
        first_kept_seq: plan.first_kept_seq,
        tokens_before: plan.tokens_before,
        tokens_after,
        messages_before: plan.messages_before,
        messages_after,
        split_turn,
        details,
    };

    if cancel.is_cancelled() {
        return Ok(CompactSessionOutcome {
            status: CompactSessionStatus::Cancelled,
            item: None,
            used_fallback: false,
        });
    }
    session.write_items(vec![item.clone()]).await?;
    session.replace_transcript(new_context).await;
    Ok(CompactSessionOutcome {
        status: CompactSessionStatus::Compacted,
        item: Some(item),
        used_fallback,
    })
}

async fn summarize_with_llm(
    entries: &[evot_engine::CompactEntry],
    plan: &evot_engine::SessionCompactPlan,
    previous_summary: Option<&str>,
    custom_instructions: Option<&str>,
    summarizer: &CompactSummarizer,
    cancel: CancellationToken,
) -> Option<String> {
    let conversation = entries[plan.summarize.clone()]
        .iter()
        .map(format_entry_for_summary)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let conversation =
        if let Some(instructions) = custom_instructions.filter(|s| !s.trim().is_empty()) {
            format!(
                "Additional user instructions for this compaction:\n{}\n\n{}",
                instructions.trim(),
                conversation
            )
        } else {
            conversation
        };

    let input = evot_engine::SummarizerInput {
        conversation,
        turn_prefix: plan.turn_prefix.as_ref().map(|range| {
            entries[range.clone()]
                .iter()
                .map(format_entry_for_summary)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        }),
        previous_summary: previous_summary.map(str::to_string),
        file_ops: plan.file_ops.clone(),
        evicted_count: plan.summarize.len(),
        completed_requests: Vec::new(),
        env_discoveries: Vec::new(),
        last_conclusion: None,
    };
    let ctx = evot_engine::SummarizerContext {
        provider: summarizer.provider.clone(),
        model: summarizer.llm.model.clone(),
        api_key: summarizer.llm.api_key.clone(),
        thinking_level: summarizer.llm.thinking_level,
        model_config: Some(crate::agent::run::runtime::build_model_config(
            summarizer.llm.protocol.clone(),
            &summarizer.llm.provider,
            &summarizer.llm.model,
            Some(&summarizer.llm.base_url),
            summarizer.llm.compat_caps,
            summarizer.llm.context_window,
            summarizer.llm.max_tokens,
            summarizer.llm.supports_image,
        )),
    };
    let mode = evot_engine::SummarizerMode::Llm {
        max_tokens: summarizer.max_tokens,
    };
    let llm_cancel = cancel.child_token();
    let summarize = mode.summarize(input, Some(&ctx), llm_cancel.clone());
    let result = tokio::select! {
        _ = cancel.cancelled() => {
            llm_cancel.cancel();
            return None;
        }
        result = tokio::time::timeout(summarizer.timeout, summarize) => result,
    };
    match result {
        Ok(Ok(output)) if !output.summary.trim().is_empty() => Some(output.summary),
        Ok(_) => None,
        Err(_) => {
            llm_cancel.cancel();
            tracing::warn!(
                stage = "compact",
                status = "summary_timeout",
                timeout_ms = summarizer.timeout.as_millis() as u64,
                "LLM compaction summary timed out; using deterministic fallback"
            );
            None
        }
    }
}

fn format_entry_for_summary(entry: &evot_engine::CompactEntry) -> String {
    let Some(message) = entry.message.as_llm() else {
        return String::new();
    };
    match message {
        evot_engine::Message::User { content, .. } => format!("User: {}", content_text(content)),
        evot_engine::Message::Assistant { content, .. } => {
            format!("Assistant: {}", content_text(content))
        }
        evot_engine::Message::ToolResult {
            tool_name, content, ..
        } => {
            format!("Tool result {tool_name}: {}", content_text(content))
        }
    }
}

fn build_new_context_items(
    context_entries: &[(u64, TranscriptItem)],
    first_kept_seq: u64,
    summary: &str,
) -> Vec<TranscriptItem> {
    let mut items = vec![compact_summary_item(summary)];
    for (seq, item) in context_entries {
        if *seq >= first_kept_seq && item.is_context_item() {
            items.push(item.clone());
        }
    }
    items
}

fn build_summary(
    entries: &[evot_engine::CompactEntry],
    plan: &evot_engine::SessionCompactPlan,
    previous_summary: Option<&str>,
    custom_instructions: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("## Goal\n");
    match previous_summary {
        Some(prev) if !prev.trim().is_empty() => {
            out.push_str("Continue from the previous compacted context.\n\n");
            out.push_str("## Previous Summary\n");
            out.push_str(prev.trim());
            out.push_str("\n\n");
        }
        _ => out.push_str("Continue the coding session using the retained recent context.\n\n"),
    }

    if let Some(instructions) = custom_instructions {
        if !instructions.trim().is_empty() {
            out.push_str("## Additional Focus\n");
            out.push_str(instructions.trim());
            out.push_str("\n\n");
        }
    }

    out.push_str("## Progress\n");
    for entry in &entries[plan.summarize.clone()] {
        append_entry_summary(&mut out, entry);
    }

    if let Some(prefix) = &plan.turn_prefix {
        out.push_str("\n## Turn Context (split turn)\n");
        for entry in &entries[prefix.clone()] {
            append_entry_summary(&mut out, entry);
        }
    }

    if !plan.file_ops.modified().is_empty() || !plan.file_ops.read_only().is_empty() {
        out.push_str("\n## Files\n");
        let modified = plan.file_ops.modified();
        if !modified.is_empty() {
            out.push_str("Modified:\n");
            for file in modified {
                out.push_str("- ");
                out.push_str(file);
                out.push('\n');
            }
        }
        let read = plan.file_ops.read_only();
        if !read.is_empty() {
            out.push_str("Read:\n");
            for file in read {
                out.push_str("- ");
                out.push_str(file);
                out.push('\n');
            }
        }
    }

    out.push_str("\n## Next Steps\nContinue from the retained recent messages below this summary.");
    out
}

fn append_entry_summary(out: &mut String, entry: &evot_engine::CompactEntry) {
    let Some(message) = entry.message.as_llm() else {
        return;
    };
    match message {
        evot_engine::Message::User { content, .. } => {
            let text = content_text(content);
            if !text.trim().is_empty() {
                out.push_str("- User: ");
                out.push_str(&truncate(&text, 240));
                out.push('\n');
            }
        }
        evot_engine::Message::Assistant { content, .. } => {
            let text = content_text(content);
            if !text.trim().is_empty() {
                out.push_str("- Assistant: ");
                out.push_str(&truncate(&text, 240));
                out.push('\n');
            }
        }
        evot_engine::Message::ToolResult {
            tool_name, content, ..
        } => {
            let text = content_text(content);
            out.push_str("- Tool result ");
            out.push_str(tool_name);
            if !text.trim().is_empty() {
                out.push_str(": ");
                out.push_str(&truncate(&text, 180));
            }
            out.push('\n');
        }
    }
}

fn content_text(content: &[evot_engine::Content]) -> String {
    content
        .iter()
        .filter_map(|content| match content {
            evot_engine::Content::Text { text } => Some(text.as_str()),
            evot_engine::Content::Thinking { thinking, .. } => Some(thinking.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in text.chars().take(max_chars) {
        out.push(ch);
    }
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}
