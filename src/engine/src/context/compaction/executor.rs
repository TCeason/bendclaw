//! Executor — applies the compaction plan to produce the final message list.

use tokio_util::sync::CancellationToken;

use super::config::truncate_summary;
use super::config::CompactionConfig;
use super::emergency;
use super::memory;
use super::remote;
use super::summarizer::mode::SummarizerContext;
use super::summarizer::serialize;
use super::summarizer::types::SummarizerInput;
use super::transforms;
use super::types::notify_compaction_phase;
use super::types::CompactionMethod;
use super::types::CompactionObserver;
use super::types::CompactionOutcome;
use super::types::CompactionPhase;
use super::types::CompactionPlan;
use super::types::CompactionState;
use super::types::CompactionStats;
use crate::context::sanitize::sanitize_tool_pairs;
use crate::context::tokens::total_tokens;
use crate::types::*;

/// Runtime controls for one compaction execution.
pub struct ExecutionOptions {
    pub use_llm_fallback: bool,
    pub observer: Option<CompactionObserver>,
    pub cancel: CancellationToken,
}

/// Execute a compaction plan against the given messages.
/// If `use_llm_fallback` is false, a failed/ineligible remote attempt uses the
/// emergency deterministic summary (overflow recovery).
/// Execute a compaction plan without lifecycle observation.
pub async fn execute(
    messages: Vec<AgentMessage>,
    plan: &CompactionPlan,
    config: &CompactionConfig,
    prev_state: Option<&CompactionState>,
    summarizer_ctx: Option<&SummarizerContext>,
    use_llm_fallback: bool,
    cancel: CancellationToken,
) -> CompactionOutcome {
    execute_with_options(
        messages,
        plan,
        config,
        prev_state,
        summarizer_ctx,
        ExecutionOptions {
            use_llm_fallback,
            observer: None,
            cancel,
        },
    )
    .await
}

/// Execute a compaction plan and report live lifecycle phases.
pub async fn execute_with_options(
    messages: Vec<AgentMessage>,
    plan: &CompactionPlan,
    config: &CompactionConfig,
    prev_state: Option<&CompactionState>,
    summarizer_ctx: Option<&SummarizerContext>,
    options: ExecutionOptions,
) -> CompactionOutcome {
    let ExecutionOptions {
        use_llm_fallback,
        observer,
        cancel,
    } = options;
    let before_message_count = messages.len();
    let before_tokens = total_tokens(&messages);

    // Step 1: Reclaim (lossless, runs on all messages)
    let (messages, current_run_reclaimed) = transforms::reclaim::run(messages);

    // Step 2: Prepare summarizer input (serialize evicted zone to text)
    let evicted = &messages[plan.evict_zone.clone()];
    let split_prefix = plan
        .split_turn
        .as_ref()
        .map(|st| &messages[st.turn_start..st.cut_at]);

    let summarizer_input = serialize::prepare_input(evicted, split_prefix, prev_state);

    // Step 3: Try provider-native remote compaction first (GPT models on the
    // Responses protocol). Any failure falls back to local text summarization.
    let mut remote_failed = false;
    let remote_outcome = match summarizer_ctx {
        Some(ctx) if remote::supports(ctx) => {
            notify_compaction_phase(&observer, CompactionPhase::Remote);
            let previous_local_summary = prev_state.and_then(|state| {
                state
                    .context_summary_message
                    .as_ref()
                    .and(state.last_summary.as_deref())
            });
            let remote_input = remote::with_previous_local_summary(evicted, previous_local_summary);
            match remote::compact(ctx, &remote_input, cancel.clone()).await {
                Ok(outcome) => Some(outcome),
                Err(remote::RemoteError::Cancelled) => {
                    // User aborted — do not compact at all.
                    return CompactionOutcome {
                        messages,
                        state: prev_state.cloned().unwrap_or_default(),
                        stats: CompactionStats::default(),
                    };
                }
                Err(remote::RemoteError::Failed(reason)) => {
                    tracing::warn!("remote compaction failed, falling back to local: {reason}");
                    remote_failed = true;
                    notify_compaction_phase(&observer, CompactionPhase::LocalFallback);
                    None
                }
            }
        }
        _ => {
            notify_compaction_phase(&observer, CompactionPhase::Local);
            None
        }
    };

    if let Some(remote_compaction) = remote_outcome {
        return assemble_remote(
            messages,
            plan,
            config,
            prev_state,
            summarizer_ctx,
            remote_compaction,
            summarizer_input,
            before_message_count,
            before_tokens,
            current_run_reclaimed,
        );
    }

    // Step 4: Generate local text summary
    let summary_text = if use_llm_fallback {
        if let Some(ctx) = summarizer_ctx {
            // LLM summarization for threshold/manual compaction
            match config
                .summarizer_mode
                .summarize(summarizer_input, Some(ctx), cancel)
                .await
            {
                Ok(out) => out.summary,
                Err(_) => {
                    // LLM failed — compaction cannot proceed
                    return CompactionOutcome {
                        messages,
                        state: prev_state.cloned().unwrap_or_default(),
                        stats: CompactionStats::default(),
                    };
                }
            }
        } else {
            emergency::summarize(&summarizer_input).summary
        }
    } else {
        // Overflow fallback is deterministic: no second model request.
        emergency::summarize(&summarizer_input).summary
    };

    let summary_text = truncate_summary(&summary_text, config.summary_max_chars);

    // Step 5: Build memory summary message
    let memory_summary_msg = AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: summary_text.clone(),
        }],
        timestamp: crate::context::now_ms(),
    });

    // Step 6: Build new state
    let mut new_state = memory::build_state(evicted, split_prefix, prev_state);
    // Store the same bounded summary used by the context and persistence event.
    // The exact context message lets the next compaction remove this copy before
    // supplying `last_summary` to the incremental summarizer.
    new_state.last_summary = Some(summary_text.clone());
    new_state.context_summary_message = Some(summary_text.clone());

    // Step 7: Assemble final messages: pinned_head + memory_summary + retained_tail
    let mut result = Vec::with_capacity(plan.pinned_head.len() + 1 + plan.retained_tail.len());
    result.extend_from_slice(&messages[plan.pinned_head.clone()]);
    result.push(memory_summary_msg);
    result.extend_from_slice(&messages[plan.retained_tail.clone()]);

    // Step 8: Sanitize orphaned tool pairs
    let result = sanitize_tool_pairs(result);

    let after_message_count = result.len();
    let after_tokens = total_tokens(&result);

    let stats = CompactionStats {
        summary: Some(summary_text.clone()),
        before_message_count,
        after_message_count,
        before_tokens,
        after_tokens,
        messages_evicted: plan.evict_zone.len(),
        current_run_reclaimed,
        method: Some(if remote_failed {
            CompactionMethod::RemoteFailedLocal
        } else {
            CompactionMethod::Local
        }),
        remote_blob_bytes: None,
    };

    CompactionOutcome {
        messages: result,
        state: new_state,
        stats,
    }
}

/// Assemble the post-compaction context around a provider-native compaction
/// item. The opaque item replaces the evicted zone; a free rule-based summary
/// rides along as the portability fallback for non-Responses providers and
/// for the incremental text summarizer.
#[allow(clippy::too_many_arguments)]
fn assemble_remote(
    messages: Vec<AgentMessage>,
    plan: &CompactionPlan,
    config: &CompactionConfig,
    prev_state: Option<&CompactionState>,
    summarizer_ctx: Option<&SummarizerContext>,
    remote_compaction: remote::RemoteCompaction,
    summarizer_input: SummarizerInput,
    before_message_count: usize,
    before_tokens: usize,
    current_run_reclaimed: usize,
) -> CompactionOutcome {
    let evicted = &messages[plan.evict_zone.clone()];
    let split_prefix = plan
        .split_turn
        .as_ref()
        .map(|st| &messages[st.turn_start..st.cut_at]);

    // Rule-based summary: zero-cost portability fallback. The Responses
    // provider replays only the opaque item; other providers render this text.
    let summary_text = emergency::summarize(&summarizer_input).summary;
    let summary_text = truncate_summary(&summary_text, config.summary_max_chars);

    let encrypted_bytes = remote_compaction.encrypted_bytes;
    let blob_message = summarizer_ctx
        .map(|ctx| remote::replacement_message(ctx, remote_compaction, summary_text.clone()))
        .unwrap_or_else(|| {
            AgentMessage::Llm(Message::Assistant {
                content: vec![Content::Text {
                    text: summary_text.clone(),
                }],
                stop_reason: StopReason::Stop,
                model: String::new(),
                provider: String::new(),
                usage: Usage::default(),
                timestamp: crate::context::now_ms(),
                error_message: None,
                response_id: None,
            })
        });

    let mut new_state = memory::build_state(evicted, split_prefix, prev_state);
    new_state.last_summary = Some(summary_text.clone());
    // No exact user text message exists in context for the next compaction to
    // dedupe; the blob message is evicted/chained like ordinary conversation.
    new_state.context_summary_message = None;

    let mut result = Vec::with_capacity(plan.pinned_head.len() + 1 + plan.retained_tail.len());
    result.extend_from_slice(&messages[plan.pinned_head.clone()]);
    result.push(blob_message);
    result.extend_from_slice(&messages[plan.retained_tail.clone()]);
    let result = sanitize_tool_pairs(result);

    let after_message_count = result.len();
    let after_tokens = total_tokens(&result);

    let stats = CompactionStats {
        summary: Some(summary_text),
        before_message_count,
        after_message_count,
        before_tokens,
        after_tokens,
        messages_evicted: plan.evict_zone.len(),
        current_run_reclaimed,
        method: Some(CompactionMethod::Remote),
        remote_blob_bytes: Some(encrypted_bytes),
    };

    CompactionOutcome {
        messages: result,
        state: new_state,
        stats,
    }
}
