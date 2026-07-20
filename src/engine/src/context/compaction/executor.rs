//! Executor — applies the compaction plan to produce the final message list.

use tokio_util::sync::CancellationToken;

use super::config::truncate_summary;
use super::config::CompactionConfig;
use super::emergency;
use super::memory;
use super::summarizer::mode::SummarizerContext;
use super::summarizer::serialize;
use super::transforms;
use super::types::CompactionOutcome;
use super::types::CompactionPlan;
use super::types::CompactionState;
use super::types::CompactionStats;
use crate::context::sanitize::sanitize_tool_pairs;
use crate::context::tokens::total_tokens;
use crate::types::*;

/// Execute a compaction plan against the given messages.
/// If `summarizer_ctx` is None, uses the emergency deterministic summary (for overflow).
pub async fn execute(
    messages: Vec<AgentMessage>,
    plan: &CompactionPlan,
    config: &CompactionConfig,
    prev_state: Option<&CompactionState>,
    summarizer_ctx: Option<&SummarizerContext>,
    cancel: CancellationToken,
) -> CompactionOutcome {
    let before_message_count = messages.len();
    let before_tokens = total_tokens(&messages);

    // Step 1: Reclaim (lossless, runs on all messages)
    let (messages, reclaim_stats) = transforms::reclaim::run(messages, config);

    // Step 2: Shrink (only on retained tail)
    let (messages, tool_results_shrunk) =
        transforms::shrink::run(messages, &plan.retained_tail, config);

    // Step 3: Prepare summarizer input (serialize evicted zone to text)
    let evicted = &messages[plan.evict_zone.clone()];
    let split_prefix = plan
        .split_turn
        .as_ref()
        .map(|st| &messages[st.turn_start..st.cut_at]);

    let summarizer_input = serialize::prepare_input(evicted, split_prefix, prev_state);

    // Step 4: Generate summary
    let summary_text = if let Some(ctx) = summarizer_ctx {
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
        // Emergency deterministic summary for overflow recovery
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
        tool_results_shrunk,
        images_downgraded: reclaim_stats.images_downgraded,
        current_run_reclaimed: reclaim_stats.current_run_reclaimed,
    };

    CompactionOutcome {
        messages: result,
        state: new_state,
        stats,
    }
}
