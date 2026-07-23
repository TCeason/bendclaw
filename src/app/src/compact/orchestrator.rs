use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::context_view::compact_summary_item;
use crate::agent::run::convert;
use crate::agent::session::Session;
use crate::conf::LlmConfig;
use crate::error::Result;
use crate::types::CompactDetails;
use crate::types::CompactReason;
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

pub use evot_engine::CompactionPhase as ManualCompactionPhase;
pub type ManualCompactionObserver = evot_engine::CompactionObserver;

#[derive(Clone)]
pub struct ManualCompactRequest {
    pub reason: CompactReason,
    pub custom_instructions: Option<String>,
    pub summary_override: Option<String>,
    pub summarizer: Option<CompactSummarizer>,
    pub settings: CompactSettings,
    pub observer: Option<ManualCompactionObserver>,
}

#[derive(Clone)]
pub struct CompactSummarizer {
    pub provider: std::sync::Arc<dyn evot_engine::provider::StreamProvider>,
    pub llm: LlmConfig,
    /// Tokens reserved for the summary request and output. The engine applies
    /// pi's output ratios: 80% for history, 50% for a split-turn prefix.
    pub reserve_tokens: u32,
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
        messages_evicted: usize,
        current_run_reclaimed: usize,
        compaction_level: usize,
        used_fallback: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        method: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_blob_bytes: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        fallback_reason: Option<String>,
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

    let observer = request.observer.clone();
    notify_phase(&observer, ManualCompactionPhase::Planning);
    let (mut app_context, mut engine_context, previous_state, expected_seq) =
        session.compaction_snapshot().await;
    if let Some(summary_message) = previous_state
        .as_ref()
        .and_then(|state| state.context_summary_message.as_deref())
    {
        if let Some(index) = engine_context
            .iter()
            .position(|message| is_exact_user_text(message, summary_message))
        {
            engine_context.remove(index);
            if index < app_context.len() {
                app_context.remove(index);
            }
        }
    }
    let compact_entries = engine_context
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, message)| evot_engine::CompactEntry {
            seq: index.saturating_add(1) as u64,
            message,
        })
        .collect::<Vec<_>>();
    let previous_summary = previous_state
        .as_ref()
        .and_then(|state| state.last_summary.as_deref());

    let plan = match evot_engine::plan_session_compaction(
        &compact_entries,
        None,
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
    let mut compaction_method: Option<String> = None;
    let mut fallback_reason: Option<String> = None;
    let mut remote_result: Option<(
        evot_engine::context::compaction::remote::RemoteCompaction,
        evot_engine::SummarizerContext,
    )> = None;

    // Manual `/compact` follows the same remote-first policy as automatic
    // compaction. Explicit summary overrides remain local by definition.
    if summary_override.is_none() {
        if let Some(summarizer) = request.summarizer.as_ref() {
            let ctx = summarizer_context(summarizer);
            if evot_engine::context::compaction::remote::supports(&ctx) {
                notify_phase(&observer, ManualCompactionPhase::Remote);
                let mut remote_messages =
                    evot_engine::context::compaction::remote::with_previous_local_summary(
                        &compact_entries[plan.summarize.clone()]
                            .iter()
                            .map(|entry| entry.message.clone())
                            .collect::<Vec<_>>(),
                        previous_state
                            .as_ref()
                            .and_then(|state| state.context_summary_message.as_ref())
                            .and(previous_summary),
                    );
                if let Some(prefix) = plan.turn_prefix.as_ref() {
                    remote_messages.extend(
                        compact_entries[prefix.clone()]
                            .iter()
                            .map(|entry| entry.message.clone()),
                    );
                }
                let remote_cancel = cancel.child_token();
                let remote_call = evot_engine::context::compaction::remote::compact(
                    &ctx,
                    &remote_messages,
                    remote_cancel.clone(),
                );
                let remote = tokio::select! {
                    _ = cancel.cancelled() => {
                        remote_cancel.cancel();
                        return Ok(CompactSessionOutcome {
                            status: CompactSessionStatus::Cancelled,
                            item: None,
                            used_fallback: false,
                        });
                    }
                    result = tokio::time::timeout(summarizer.timeout, remote_call) => result,
                };
                match remote {
                    Ok(Ok(compaction)) => {
                        compaction_method = Some("remote".into());
                        remote_result = Some((compaction, ctx));
                    }
                    Ok(Err(evot_engine::context::compaction::remote::RemoteError::Cancelled)) => {
                        return Ok(CompactSessionOutcome {
                            status: CompactSessionStatus::Cancelled,
                            item: None,
                            used_fallback: false,
                        });
                    }
                    Ok(Err(evot_engine::context::compaction::remote::RemoteError::Failed(
                        error,
                    ))) => {
                        tracing::warn!(stage = "compact", status = "remote_failed", %error,
                            "manual remote compaction failed; falling back to local summary");
                        compaction_method = Some("remote_failed_local".into());
                        fallback_reason = Some(
                            evot_engine::context::compaction::remote::bounded_fallback_reason(
                                &error,
                            ),
                        );
                        notify_phase(&observer, ManualCompactionPhase::LocalFallback);
                    }
                    Err(_) => {
                        remote_cancel.cancel();
                        tracing::warn!(
                            stage = "compact",
                            status = "remote_timeout",
                            "manual remote compaction timed out; falling back to local summary"
                        );
                        compaction_method = Some("remote_failed_local".into());
                        fallback_reason = Some(format!(
                            "remote compaction timed out after {} seconds",
                            summarizer.timeout.as_secs()
                        ));
                        notify_phase(&observer, ManualCompactionPhase::LocalFallback);
                    }
                }
            } else {
                compaction_method = Some("local".into());
                fallback_reason =
                    evot_engine::context::compaction::remote::unavailable_reason(&ctx);
                notify_phase(&observer, ManualCompactionPhase::Local);
            }
        }
    }

    let summary = if let Some(summary) = summary_override {
        notify_phase(&observer, ManualCompactionPhase::Local);
        compaction_method = Some("local".into());
        summary
    } else if remote_result.is_some() {
        // Remote state is authoritative for compatible future turns. Keep a
        // deterministic readable summary for exports and model switching.
        build_summary(
            &compact_entries,
            &plan,
            previous_summary,
            request.custom_instructions.as_deref(),
        )
    } else if let Some(summarizer) = request.summarizer.as_ref() {
        if compaction_method.is_none() {
            notify_phase(&observer, ManualCompactionPhase::Local);
        }
        let generated = summarize_with_llm(
            &compact_entries,
            &plan,
            previous_summary,
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
            Some(summary) => {
                if compaction_method.is_none() {
                    compaction_method = Some("local".into());
                }
                summary
            }
            None => {
                used_fallback = true;
                if compaction_method.is_none() {
                    compaction_method = Some("local".into());
                }
                build_summary(
                    &compact_entries,
                    &plan,
                    previous_summary,
                    request.custom_instructions.as_deref(),
                )
            }
        }
    } else {
        notify_phase(&observer, ManualCompactionPhase::Local);
        compaction_method = Some("local".into());
        build_summary(
            &compact_entries,
            &plan,
            previous_summary,
            request.custom_instructions.as_deref(),
        )
    };
    let summary_item = compact_summary_item(&summary);
    let remote_blob_bytes = remote_result
        .as_ref()
        .map(|(compaction, _)| compaction.encrypted_bytes);
    let summary_message = match remote_result {
        Some((compaction, ctx)) => evot_engine::context::compaction::remote::replacement_message(
            &ctx,
            compaction,
            summary.clone(),
        ),
        None => convert::agent_message_from_transcript(&summary_item),
    };
    let mut new_context = vec![summary_item];
    new_context.extend(app_context[plan.first_kept..].iter().cloned());
    let mut new_engine_context = vec![summary_message];
    new_engine_context.extend(engine_context[plan.first_kept..].iter().cloned());
    let messages_after = new_context.len();
    let tokens_after = evot_engine::context::total_tokens(&new_engine_context);

    let mut details = CompactDetails::default();
    if let Some(previous) = previous_state.as_ref() {
        details.read_files = previous.file_ops.read.iter().cloned().collect();
        details.modified_files = previous.file_ops.modified().into_iter().cloned().collect();
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
    details.method = compaction_method.clone();
    details.remote_blob_bytes = remote_blob_bytes;
    details.fallback_reason = fallback_reason;

    let mut state = previous_state.unwrap_or_default();
    state
        .file_ops
        .read
        .extend(plan.file_ops.read.iter().cloned());
    state
        .file_ops
        .written
        .extend(plan.file_ops.written.iter().cloned());
    state
        .file_ops
        .edited
        .extend(plan.file_ops.edited.iter().cloned());
    state.timestamp = evot_engine::now_ms();
    state.generation = state.generation.saturating_add(1);
    state.last_summary = Some(summary.clone());
    state.context_summary_message = if compaction_method.as_deref() == Some("remote") {
        None
    } else {
        new_engine_context.first().and_then(exact_user_text)
    };

    let item = TranscriptItem::Compact {
        id: crate::types::new_id(),
        created_at: state.timestamp,
        reason: request.reason,
        summary,
        tokens_before: plan.tokens_before,
        tokens_after,
        messages_before: plan.messages_before,
        messages_after,
        messages: new_context.clone(),
        engine_messages: new_engine_context,
        state: Box::new(state),
        details,
    };

    if cancel.is_cancelled() {
        return Ok(CompactSessionOutcome {
            status: CompactSessionStatus::Cancelled,
            item: None,
            used_fallback: false,
        });
    }
    session
        .write_compact(item.clone(), new_context, expected_seq)
        .await?;
    notify_phase(&observer, ManualCompactionPhase::Complete);
    Ok(CompactSessionOutcome {
        status: CompactSessionStatus::Compacted,
        item: Some(item),
        used_fallback,
    })
}

fn notify_phase(observer: &Option<ManualCompactionObserver>, phase: ManualCompactionPhase) {
    evot_engine::context::compaction::types::notify_compaction_phase(observer, phase);
}

fn is_exact_user_text(message: &evot_engine::AgentMessage, expected: &str) -> bool {
    exact_user_text(message).is_some_and(|text| text == expected)
}

fn exact_user_text(message: &evot_engine::AgentMessage) -> Option<String> {
    let evot_engine::AgentMessage::Llm(evot_engine::Message::User { content, .. }) = message else {
        return None;
    };
    match content.as_slice() {
        [evot_engine::Content::Text { text }] => Some(text.clone()),
        _ => None,
    }
}

fn summarizer_context(summarizer: &CompactSummarizer) -> evot_engine::SummarizerContext {
    evot_engine::SummarizerContext {
        provider: summarizer.provider.clone(),
        model: summarizer.llm.model.clone(),
        api_key: summarizer.llm.api_key.clone(),
        thinking_level: summarizer.llm.thinking_level,
        system_prompt: String::new(),
        tools: vec![],
        max_tokens: Some(summarizer.reserve_tokens),
        cache_config: evot_engine::CacheConfig::default(),
        prompt_cache_key: None,
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
    }
}

async fn summarize_with_llm(
    entries: &[evot_engine::CompactEntry],
    plan: &evot_engine::SessionCompactPlan,
    previous_summary: Option<&str>,
    custom_instructions: Option<&str>,
    summarizer: &CompactSummarizer,
    cancel: CancellationToken,
) -> Option<String> {
    let conversation = serialize_entries(entries, plan.summarize.clone());

    let input = evot_engine::SummarizerInput {
        conversation,
        turn_prefix: plan
            .turn_prefix
            .as_ref()
            .map(|range| serialize_entries(entries, range.clone())),
        previous_summary: previous_summary.map(str::to_string),
        custom_instructions: custom_instructions.map(str::to_string),
        file_ops: plan.file_ops.clone(),
        evicted_count: plan.summarize.len(),
        completed_requests: Vec::new(),
        env_discoveries: Vec::new(),
        last_conclusion: None,
    };
    let ctx = summarizer_context(summarizer);
    let mode = evot_engine::SummarizerMode::Llm {
        reserve_tokens: summarizer.reserve_tokens,
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

fn serialize_entries(
    entries: &[evot_engine::CompactEntry],
    range: std::ops::Range<usize>,
) -> String {
    let messages = entries[range]
        .iter()
        .map(|entry| entry.message.clone())
        .collect::<Vec<_>>();
    evot_engine::context::compaction::summarizer::serialize::serialize_messages(&messages)
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
