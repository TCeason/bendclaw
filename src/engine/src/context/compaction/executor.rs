//! Executor — applies the compaction plan to produce the final message list.

use tokio_util::sync::CancellationToken;

use super::config::CompactionConfig;
use super::marker;
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

    // Step 4: Generate summary via configured mode
    let output = config
        .summarizer_mode
        .summarize(summarizer_input, summarizer_ctx, cancel)
        .await;

    let summary_text = match output {
        Ok(out) => out.summary,
        Err(_) => {
            // LLM failed — compaction cannot proceed
            // Return original messages unchanged (controller handles the error)
            return CompactionOutcome {
                messages,
                state: prev_state.cloned().unwrap_or_default(),
                stats: CompactionStats::default(),
            };
        }
    };

    // Step 5: Build marker message
    let marker_msg = AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: summary_text.clone(),
        }],
        timestamp: crate::context::now_ms(),
    });

    // Step 6: Build new state
    let mut new_state = marker::build_state(evicted, split_prefix, prev_state);
    // Store summary for incremental updates (capped at char boundary)
    let capped_summary = if summary_text.len() > config.summary_max_chars {
        let mut end = config.summary_max_chars;
        while end > 0 && !summary_text.is_char_boundary(end) {
            end -= 1;
        }
        summary_text[..end].to_string()
    } else {
        summary_text
    };
    new_state.last_summary = Some(capped_summary);

    // Step 7: Assemble final messages: pinned_head + marker + retained_tail
    let mut result = Vec::with_capacity(plan.pinned_head.len() + 1 + plan.retained_tail.len());
    result.extend_from_slice(&messages[plan.pinned_head.clone()]);
    result.push(marker_msg);
    result.extend_from_slice(&messages[plan.retained_tail.clone()]);

    // Step 8: Sanitize orphaned tool pairs
    let result = sanitize_tool_pairs(result);

    let after_message_count = result.len();
    let after_tokens = total_tokens(&result);

    let stats = CompactionStats {
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
