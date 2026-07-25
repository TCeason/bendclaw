//! Context tracking, configuration, and execution limits.

use serde::Deserialize;
use serde::Serialize;

use super::tokens::message_tokens;
use super::tokens::tool_definition_tokens;
use super::tokens::total_tokens;
use crate::provider::ToolDefinition;
use crate::types::*;

// ---------------------------------------------------------------------------
// Context tracking (real usage + estimates)
// ---------------------------------------------------------------------------

/// Measures current context size, anchored on the provider's own token count.
///
/// The accurate, model-specific size always comes from the latest assistant
/// `usage` already embedded in the transcript — not from any local tokenizer.
/// Because that anchor lives in the message list itself, the measurement is
/// correct immediately on resume (no in-memory state to lose) and is identical
/// across all models. A cheap byte approximation only sizes the small trailing
/// delta since that response, and serves as a floor before the first response.
pub struct ContextTracker {
    /// Timestamp of the latest compaction, when one happened in this run.
    ///
    /// Assistant messages at or before this point carry usage describing the
    /// pre-compaction (larger) context, so they cannot anchor the estimate.
    /// Newer responses can, which keeps provider-accurate counts available
    /// immediately after a compaction. Mirrors pi's compaction-boundary check.
    /// This is transient run state — correctly absent on resume, where the
    /// resolved context view already exposes a valid post-compaction anchor.
    compacted_at: Option<u64>,
    system_tool_overhead_tokens: usize,
}

impl ContextTracker {
    pub fn new() -> Self {
        Self {
            compacted_at: None,
            system_tool_overhead_tokens: 0,
        }
    }

    /// Record fixed request overhead that compaction cannot reduce.
    pub fn record_request_overhead(&mut self, system_prompt: &str, tools: &[ToolDefinition]) {
        self.system_tool_overhead_tokens =
            crate::context::estimate_tokens(system_prompt) + tool_definition_tokens(tools);
    }

    pub fn system_tool_overhead_tokens(&self) -> usize {
        self.system_tool_overhead_tokens
    }

    /// Record the compaction boundary. Assistant usage from at or before this
    /// timestamp is stale and cannot anchor the estimate.
    pub fn record_compaction_done(&mut self, compacted_at: u64) {
        self.compacted_at = Some(compacted_at);
    }

    /// Measure current context size: provider anchor + pi-compatible local
    /// estimate of the trailing delta since that response.
    ///
    /// The latest valid provider usage is the anchor; only later messages are
    /// estimated locally. Without an anchor (including immediately after
    /// compaction), include fixed request overhead in the local estimate.
    pub fn estimate_context_tokens(&self, messages: &[AgentMessage]) -> usize {
        self.estimate_context_tokens_for_model(messages, None, None)
    }

    /// Measure context for the model about to receive the request.
    ///
    /// Provider token counts are model-specific. After a model switch, reusing
    /// the previous model's usage can severely undercount the same serialized
    /// history, so only a matching model may anchor the estimate. With no
    /// matching anchor, conservatively estimate the complete message list.
    pub fn estimate_context_tokens_for_model(
        &self,
        messages: &[AgentMessage],
        target_provider: Option<&str>,
        target_model: Option<&str>,
    ) -> usize {
        self.estimate_context_tokens_from_anchor_for_model(messages, target_provider, target_model)
            .unwrap_or_else(|| {
                total_tokens(messages).saturating_add(self.system_tool_overhead_tokens)
            })
    }

    /// Estimate from a real provider usage anchor, or return `None` when no
    /// valid post-compaction response exists. Compaction checks use this form to
    /// match pi: zero-usage/error responses may fall back to prior usage, but
    /// must not trigger from a full-history local estimate alone.
    pub fn estimate_context_tokens_from_anchor_for_model(
        &self,
        messages: &[AgentMessage],
        target_provider: Option<&str>,
        target_model: Option<&str>,
    ) -> Option<usize> {
        let (baseline, idx) =
            latest_provider_anchor(messages, target_provider, target_model, self.compacted_at)?;
        let trailing: usize = messages[idx + 1..].iter().map(message_tokens).sum();
        Some(baseline + trailing)
    }

    /// Build a budget snapshot from the current tracker state and config.
    pub fn budget_snapshot(
        &self,
        messages: &[AgentMessage],
        ctx_config: Option<&ContextConfig>,
        target_provider: Option<&str>,
        target_model: Option<&str>,
    ) -> ContextBudgetSnapshot {
        let estimated_tokens =
            self.estimate_context_tokens_for_model(messages, target_provider, target_model);
        let (system_prompt_tokens, budget_tokens, context_window) = ctx_config
            .map(|c| {
                (
                    c.system_prompt_tokens,
                    c.max_context_tokens.saturating_sub(c.system_prompt_tokens),
                    c.max_context_tokens,
                )
            })
            .unwrap_or((0, 0, 0));
        ContextBudgetSnapshot {
            estimated_tokens,
            budget_tokens,
            system_prompt_tokens,
            tool_definition_tokens: self
                .system_tool_overhead_tokens
                .saturating_sub(system_prompt_tokens),
            context_window,
        }
    }
}

/// Latest assistant usage that can anchor a context estimate, as
/// `(context_tokens, index)`.
///
/// Uses provider total usage, falling back to normalized usage buckets. Rejects
/// responses from a different provider/model (their counts do not describe this
/// model's serialization) and responses at or before the compaction boundary
/// (their counts describe the pre-compaction context).
fn latest_provider_anchor(
    messages: &[AgentMessage],
    target_provider: Option<&str>,
    target_model: Option<&str>,
    compacted_at: Option<u64>,
) -> Option<(usize, usize)> {
    let mut latest_prefix_timestamp = 0;
    let mut latest_anchor = None;

    for (idx, message) in messages.iter().enumerate() {
        let Some(message) = message.as_llm() else {
            continue;
        };
        let timestamp = message_timestamp(message);
        if let Message::Assistant {
            usage,
            provider,
            model,
            stop_reason,
            ..
        } = message
        {
            let usage_applies_to_prefix = timestamp >= latest_prefix_timestamp;
            let matches_target = target_provider.is_none_or(|target| provider == target)
                && target_model.is_none_or(|target| model == target);
            let is_after_compaction = compacted_at.is_none_or(|boundary| timestamp > boundary);
            let anchor = usage.context_tokens() as usize;
            if usage_applies_to_prefix
                && matches_target
                && is_after_compaction
                && *stop_reason != StopReason::Aborted
                && *stop_reason != StopReason::Error
                && anchor > 0
            {
                latest_anchor = Some((anchor, idx));
            }
        }
        latest_prefix_timestamp = latest_prefix_timestamp.max(timestamp);
    }

    latest_anchor
}

fn message_timestamp(message: &Message) -> u64 {
    match message {
        Message::User { timestamp, .. }
        | Message::Assistant { timestamp, .. }
        | Message::ToolResult { timestamp, .. } => *timestamp,
    }
}

impl Default for ContextTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Context budget snapshot
// ---------------------------------------------------------------------------

/// Point-in-time context budget snapshot, sourced from `ContextTracker`.
/// Shared by `LlmCallStart` and `ContextCompactionStart` events.
#[derive(Debug, Clone)]
pub struct ContextBudgetSnapshot {
    pub estimated_tokens: usize,
    pub budget_tokens: usize,
    pub system_prompt_tokens: usize,
    pub tool_definition_tokens: usize,
    pub context_window: usize,
}

// ---------------------------------------------------------------------------
// Compaction budget state (runtime, passed into compact)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Context configuration
// ---------------------------------------------------------------------------

/// Configuration for context management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Usable context tokens — the model's full context window.
    /// Output headroom is reserved separately via
    /// `CompactionConfig::reserve_tokens`, so this is NOT pre-discounted.
    pub max_context_tokens: usize,
    /// Tokens reserved for the system prompt
    pub system_prompt_tokens: usize,
    /// Output headroom reserved before compaction triggers. `None` uses
    /// [`crate::context::DEFAULT_RESERVE_TOKENS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve_tokens: Option<usize>,
    /// Token budget for the retained tail. `None` uses
    /// [`crate::context::DEFAULT_KEEP_RECENT_TOKENS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_recent_tokens: Option<usize>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 100_000,
            system_prompt_tokens: 4_000,
            reserve_tokens: None,
            keep_recent_tokens: None,
        }
    }
}

impl ContextConfig {
    /// Derive a context config from a model's context window size.
    ///
    /// Uses the full context window as the budget. Output headroom is the
    /// sole responsibility of `CompactionConfig::reserve_tokens`, so the
    /// window is not pre-discounted here (avoids double-counting headroom,
    /// which previously made compaction trigger at ~70% of the real window
    /// while the footer measured against the discounted 80% value).
    pub fn from_context_window(context_window: u32) -> Self {
        Self {
            max_context_tokens: context_window as usize,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Execution limits
// ---------------------------------------------------------------------------

/// Execution limits for the agent loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLimits {
    /// Maximum number of turns (LLM calls)
    pub max_turns: usize,
    /// Maximum total tokens consumed
    pub max_total_tokens: usize,
    /// Maximum wall-clock time
    pub max_duration: std::time::Duration,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 50,
            max_total_tokens: 1_000_000,
            max_duration: std::time::Duration::from_secs(600),
        }
    }
}

/// Accumulates wall-clock time the agent spent blocked waiting for the user
/// (e.g. the `ask_user` tool sitting idle until a choice is made). This time is
/// excluded from execution-limit duration accounting so a slow human answer
/// never trips `max_duration` — the limit should bound the agent's own work,
/// not how long a person took to respond.
///
/// Cheap to clone: all clones share one atomic counter.
#[derive(Clone, Default)]
pub struct IdleClock {
    accumulated_ms: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl IdleClock {
    pub fn new() -> Self {
        Self {
            accumulated_ms: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Total idle time accumulated so far.
    pub fn accumulated(&self) -> std::time::Duration {
        std::time::Duration::from_millis(
            self.accumulated_ms
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Begin an idle interval. The returned guard records the elapsed time into
    /// the clock when dropped, so the interval is counted on normal completion,
    /// cancellation, and panic alike.
    pub fn pause(&self) -> IdlePause {
        IdlePause {
            clock: self.accumulated_ms.clone(),
            started: std::time::Instant::now(),
        }
    }
}

/// RAII guard for an idle interval. Adds its lifetime to the [`IdleClock`] on
/// drop. Created via [`IdleClock::pause`].
pub struct IdlePause {
    clock: std::sync::Arc<std::sync::atomic::AtomicU64>,
    started: std::time::Instant,
}

impl Drop for IdlePause {
    fn drop(&mut self) {
        let elapsed_ms = self.started.elapsed().as_millis() as u64;
        self.clock
            .fetch_add(elapsed_ms, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Tracks execution state against limits
pub struct ExecutionTracker {
    pub limits: ExecutionLimits,
    pub turns: usize,
    pub tokens_used: usize,
    pub started_at: std::time::Instant,
    idle_clock: IdleClock,
}

impl ExecutionTracker {
    pub fn new(limits: ExecutionLimits) -> Self {
        Self::with_idle_clock(limits, IdleClock::new())
    }

    /// Build a tracker that shares `idle_clock` with the tool layer, so time
    /// spent waiting on the user is excluded from the duration limit.
    pub fn with_idle_clock(limits: ExecutionLimits, idle_clock: IdleClock) -> Self {
        Self {
            limits,
            turns: 0,
            tokens_used: 0,
            started_at: std::time::Instant::now(),
            idle_clock,
        }
    }

    /// Handle to the shared idle clock, for handing to the tool layer.
    pub fn idle_clock(&self) -> IdleClock {
        self.idle_clock.clone()
    }

    /// Wall-clock time elapsed minus time spent blocked waiting on the user.
    fn active_elapsed(&self) -> std::time::Duration {
        self.started_at
            .elapsed()
            .saturating_sub(self.idle_clock.accumulated())
    }

    pub fn record_turn(&mut self, tokens: usize) {
        self.turns += 1;
        self.tokens_used += tokens;
    }

    /// Check if any limit has been exceeded. Returns the reason if so.
    pub fn check_limits(&self) -> Option<String> {
        if self.turns >= self.limits.max_turns {
            return Some(format!(
                "Max turns reached ({}/{})",
                self.turns, self.limits.max_turns
            ));
        }
        if self.tokens_used >= self.limits.max_total_tokens {
            return Some(format!(
                "Max tokens reached ({}/{})",
                self.tokens_used, self.limits.max_total_tokens
            ));
        }
        let elapsed = self.active_elapsed();
        if elapsed >= self.limits.max_duration {
            return Some(format!(
                "Max duration reached ({:.0}s/{:.0}s)",
                elapsed.as_secs_f64(),
                self.limits.max_duration.as_secs_f64()
            ));
        }
        None
    }
}
