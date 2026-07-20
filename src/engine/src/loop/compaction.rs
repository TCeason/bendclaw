//! Context compaction integration with the agent loop.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::config::AgentLoopConfig;
use crate::context::AfterResponseAction;
use crate::context::CompactionController;
use crate::context::CompactionResponse;
use crate::context::ContextTracker;
use crate::context::ModelId;
use crate::context::SummarizerContext;
use crate::context::UsageSnapshot;
use crate::types::*;

/// User-visible message emitted when overflow recovery is exhausted.
const OVERFLOW_EXHAUSTED_MESSAGE: &str =
    "Context overflow recovery failed after one compact-and-retry attempt. \
     Try reducing context or switching to a larger-context model.";

const PREFLIGHT_CONTEXT_MESSAGE: &str =
    "Context remains at or above the model window after compaction. The request was not sent because it would leave no usable output budget. Reduce context or switch to a larger-context model.";

/// Run the post-response compaction policy for one assistant message.
pub(super) async fn post_response_compaction(
    controller: &mut Option<CompactionController>,
    tracker: &mut ContextTracker,
    messages: &mut Vec<AgentMessage>,
    assistant_message: &Message,
    config: &AgentLoopConfig,
    cancel: CancellationToken,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> bool {
    let ctrl = match controller.as_mut() {
        Some(ctrl) => ctrl,
        None => {
            emit_accepted_message(tx, assistant_message);
            return false;
        }
    };

    let usage = match usage_snapshot_from_message(assistant_message) {
        Some(usage) => usage,
        None => {
            emit_accepted_message(tx, assistant_message);
            return false;
        }
    };

    let current_model = ModelId {
        provider: usage.model.provider.clone(),
        model: config.model.clone(),
    };
    let summarizer_ctx = SummarizerContext {
        provider: Arc::clone(&config.provider),
        model: config.model.clone(),
        api_key: config.api_key.clone(),
        thinking_level: config.thinking_level,
        model_config: config.model_config.clone(),
    };

    let response = ctrl
        .after_response(
            messages,
            &usage,
            &current_model,
            Some(&summarizer_ctx),
            cancel.clone(),
        )
        .await;

    // A non-overflow provider error (e.g. "overloaded", 5xx) carries no usable
    // token counts, so the trigger skips it. Fall back to a local estimate so a
    // near-full session can still compact before the next attempt instead of
    // staying stuck over budget. Mirrors pi-mono's `_checkCompaction` Case 2.
    let response = if response.action == AfterResponseAction::Continue
        && response.stats.is_none()
        && is_non_overflow_error(assistant_message)
    {
        let estimated_tokens = tracker.estimate_context_tokens_for_model(
            messages,
            target_provider(config),
            Some(&config.model),
        );
        ctrl.compact_on_estimate(messages, estimated_tokens, Some(&summarizer_ctx), cancel)
            .await
    } else {
        response
    };

    let should_retry = response.action == AfterResponseAction::Retry;
    if !should_retry {
        emit_accepted_message(tx, assistant_message);
    }
    emit_compaction_events(ctrl, tracker, messages, &response, tx);

    if !should_retry {
        if let Message::Assistant { stop_reason, .. } = assistant_message {
            if *stop_reason != StopReason::Error && *stop_reason != StopReason::Aborted {
                ctrl.on_success();
            }
        }
    }

    should_retry
}

fn emit_accepted_message(tx: &mpsc::UnboundedSender<AgentEvent>, message: &Message) {
    tx.send(AgentEvent::MessageEnd {
        message: AgentMessage::Llm(message.clone()),
    })
    .ok();
}

/// Run the pre-prompt compaction policy before the first LLM call of a run.
///
/// Proactively compacts when the estimated context already exceeds the trigger
/// threshold (e.g. resuming a near-full session, or after the user aborted a
/// large turn). This avoids sending an oversized request that would only be
/// recovered reactively via overflow handling.
pub(super) async fn pre_prompt_compaction(
    controller: &mut Option<CompactionController>,
    tracker: &mut ContextTracker,
    messages: &mut Vec<AgentMessage>,
    config: &AgentLoopConfig,
    cancel: CancellationToken,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> bool {
    let ctrl = match controller.as_mut() {
        Some(ctrl) => ctrl,
        None => return true,
    };

    let summarizer_ctx = SummarizerContext {
        provider: Arc::clone(&config.provider),
        model: config.model.clone(),
        api_key: config.api_key.clone(),
        thinking_level: config.thinking_level,
        model_config: config.model_config.clone(),
    };

    loop {
        let estimated_tokens = tracker.estimate_context_tokens_for_model(
            messages,
            target_provider(config),
            Some(&config.model),
        );
        let response = ctrl
            .compact_on_estimate(
                messages,
                estimated_tokens,
                Some(&summarizer_ctx),
                cancel.clone(),
            )
            .await;

        emit_compaction_events(ctrl, tracker, messages, &response, tx);

        if cancel.is_cancelled() {
            return false;
        }

        let remaining = tracker.estimate_context_tokens_for_model(
            messages,
            target_provider(config),
            Some(&config.model),
        );
        if remaining <= ctrl.config().trigger_threshold() {
            return true;
        }
        if response.stats.is_none() || remaining >= estimated_tokens {
            // Keep pi's permissive behavior for threshold-only estimates: a
            // model-specific tokenizer or stale catalog may be conservative.
            // Once the estimate reaches the full window, however, forwarding
            // would clamp output to one token. Stop locally instead of turning
            // a failed compaction into a successful empty provider response.
            if remaining >= ctrl.config().context_window {
                tx.send(AgentEvent::Error {
                    error: AgentErrorInfo {
                        kind: AgentErrorKind::Runtime,
                        message: PREFLIGHT_CONTEXT_MESSAGE.to_string(),
                    },
                })
                .ok();
                return false;
            }
            return true;
        }
    }
}

/// Emit compaction lifecycle events and the overflow-exhausted notice.
///
/// Shared by post-response and pre-prompt compaction so both surface the same
/// observability events and persist via the app layer's compact orchestrator.
fn emit_compaction_events(
    ctrl: &CompactionController,
    tracker: &mut ContextTracker,
    messages: &[AgentMessage],
    response: &CompactionResponse,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) {
    if let Some(ref stats) = response.stats {
        let reason = response
            .reason
            .unwrap_or(crate::context::CompactReason::Threshold);
        let will_retry = response.action == AfterResponseAction::Retry;
        tx.send(AgentEvent::ContextCompactionStarted {
            reason,
            estimated_tokens: response.context_tokens.unwrap_or(stats.before_tokens),
            context_window: ctrl.config().context_window,
            reserve_tokens: ctrl.config().reserve_tokens,
            trigger_threshold: ctrl.config().trigger_threshold(),
            will_retry,
        })
        .ok();
        tx.send(AgentEvent::ContextCompactionEnd {
            reason,
            stats: stats.clone(),
            messages: messages.to_vec(),
            state: ctrl.state().clone(),
            summary: stats.summary.clone(),
            context_window: ctrl.config().context_window,
            will_retry,
        })
        .ok();
        if stats.before_tokens > stats.after_tokens {
            tracker.record_compaction_done();
        }
    }

    if response.overflow_exhausted {
        tx.send(AgentEvent::Error {
            error: AgentErrorInfo {
                kind: AgentErrorKind::Runtime,
                message: OVERFLOW_EXHAUSTED_MESSAGE.to_string(),
            },
        })
        .ok();
    }
}

fn target_provider(config: &AgentLoopConfig) -> Option<&str> {
    config
        .model_config
        .as_ref()
        .map(|model| model.provider.as_str())
}

/// Whether the response is a provider error that is *not* a context overflow.
///
/// Overflow errors are handled by the trigger's dedicated compact-and-retry
/// path. Other errors (overloaded, 5xx, network) carry no usable usage, so the
/// caller falls back to an estimate-based threshold compaction.
fn is_non_overflow_error(message: &Message) -> bool {
    matches!(
        message,
        Message::Assistant { stop_reason: StopReason::Error, error_message, .. }
            if !error_message
                .as_deref()
                .is_some_and(crate::provider::error::is_context_overflow_message)
    )
}

fn usage_snapshot_from_message(message: &Message) -> Option<UsageSnapshot> {
    if let Message::Assistant {
        usage,
        stop_reason,
        model,
        provider,
        timestamp,
        error_message,
        ..
    } = message
    {
        Some(UsageSnapshot {
            input: usage.input as usize,
            cache_read: usage.cache_read as usize,
            cache_write: usage.cache_write as usize,
            output: usage.output as usize,
            total_tokens: usage.total_tokens as usize,
            model: ModelId {
                provider: provider.clone(),
                model: model.clone(),
            },
            timestamp: *timestamp,
            stop_reason: stop_reason.clone(),
            error_message: error_message.clone(),
        })
    } else {
        None
    }
}
