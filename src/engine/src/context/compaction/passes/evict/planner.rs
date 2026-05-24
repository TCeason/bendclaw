use super::bounds::eviction_bounds;
use super::bounds::token_eviction_bounds;
use super::units::safe_message_windows;
use super::units::span_score;
use super::EvictionPlan;
use super::Span;
use crate::context::compaction::config::CompactionConfig;
use crate::context::compaction::marker;
use crate::context::tokens::message_tokens;
use crate::types::AgentMessage;

pub(super) fn token_plan(
    messages: &[AgentMessage],
    config: &CompactionConfig,
    current_tokens: usize,
) -> Option<EvictionPlan> {
    if current_tokens <= config.budget_tokens {
        return None;
    }

    let bounds = token_eviction_bounds(messages, config)?;
    let mut best: Option<(EvictionPlan, usize, usize)> = None;

    for start in bounds.start..bounds.end {
        let mut freed = 0usize;
        for end in (start + 1)..=bounds.end {
            freed += message_tokens(&messages[end - 1]);
            let span = Span { start, end };
            let after_without_marker = current_tokens.saturating_sub(freed);
            if after_without_marker > config.budget_tokens {
                continue;
            }

            let marker = marker::build_full_marker(messages, span.len());
            let marker_tokens = message_tokens(&marker);
            let (marker, action_after_tokens, final_tokens) = if marker_tokens < freed
                && after_without_marker.saturating_add(marker_tokens) <= config.budget_tokens
            {
                (
                    Some(marker),
                    marker_tokens,
                    after_without_marker + marker_tokens,
                )
            } else {
                (None, 0, after_without_marker)
            };

            let slack = config.budget_tokens - final_tokens;
            let plan = EvictionPlan {
                span,
                marker,
                after_tokens: action_after_tokens,
            };
            if best
                .as_ref()
                .map(|(_, best_len, best_slack)| {
                    span.len() < *best_len || (span.len() == *best_len && slack < *best_slack)
                })
                .unwrap_or(true)
            {
                best = Some((plan, span.len(), slack));
            }
            break;
        }
    }

    best.map(|(plan, _, _)| plan)
}

pub(super) fn message_limit_plan(
    messages: &[AgentMessage],
    config: &CompactionConfig,
    target_len: usize,
) -> Option<EvictionPlan> {
    let bounds = eviction_bounds(messages, config)?;
    let need_remove = messages.len().saturating_sub(target_len);
    if need_remove == 0 {
        return None;
    }

    safe_message_windows(messages, bounds, need_remove)
        .into_iter()
        .filter_map(|span| {
            let dropped_tokens: usize = messages[span.start..span.end]
                .iter()
                .map(message_tokens)
                .sum();
            let marker = marker::build_full_marker(messages, span.len());
            let marker_tokens = message_tokens(&marker);
            let can_add_marker = span.len() > need_remove && marker_tokens <= dropped_tokens;
            let marker = can_add_marker.then_some(marker);
            let marker_tokens = if marker.is_some() { marker_tokens } else { 0 };
            let final_len = messages.len() - span.len() + usize::from(marker.is_some());
            (final_len <= target_len).then_some((span, marker, marker_tokens, final_len))
        })
        .min_by_key(|(span, marker, _, final_len)| {
            (
                usize::from(marker.is_none()),
                span_score(messages, *span),
                target_len - final_len,
                span.len(),
                span.start,
            )
        })
        .map(|(span, marker, marker_tokens, _)| EvictionPlan {
            span,
            marker,
            after_tokens: marker_tokens,
        })
}
