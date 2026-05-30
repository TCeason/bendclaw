//! Context compaction integration with the agent loop.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::config::AgentLoopConfig;
use crate::context::AfterResponseAction;
use crate::context::CompactionController;
use crate::context::ContextTracker;
use crate::context::ModelId;
use crate::context::SummarizerContext;
use crate::context::UsageSnapshot;
use crate::types::*;

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
        None => return false,
    };

    let usage = match usage_snapshot_from_message(assistant_message) {
        Some(usage) => usage,
        None => return false,
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
    };

    let response = ctrl
        .after_response(
            messages,
            &usage,
            &current_model,
            Some(&summarizer_ctx),
            cancel,
        )
        .await;

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
            messages: messages.clone(),
            summary: stats.summary.clone(),
            context_window: ctrl.config().context_window,
            will_retry,
        })
        .ok();
        if stats.before_tokens > stats.after_tokens {
            tracker.record_compaction_done();
        }
    }

    let should_retry = response.action == AfterResponseAction::Retry;
    if !should_retry {
        if let Message::Assistant { stop_reason, .. } = assistant_message {
            if *stop_reason == StopReason::Stop {
                ctrl.on_success();
            }
        }
    }

    should_retry
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
