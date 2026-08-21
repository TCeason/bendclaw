//! Context window tracking — usage anchors, budget snapshots, and context config.

use serde::Deserialize;
use serde::Serialize;

use super::tokens::estimate_tokens;
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
            estimate_tokens(system_prompt) + tool_definition_tokens(tools);
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
        let (system_prompt_tokens, budget_tokens, max_context_tokens) = ctx_config
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
            // Display value: the model's documented total window. May exceed
            // `budget_tokens` when the provider reserves output headroom
            // inside the advertised window (e.g. DeepSeek V4: 1M total vs
            // 616K real input budget).
            context_window: ctx_config
                .and_then(|c| c.advertised_context_window)
                .unwrap_or(max_context_tokens),
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
// Context configuration
// ---------------------------------------------------------------------------

/// Configuration for context management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Effective request-input limit for the selected model and reasoning mode.
    pub max_context_tokens: usize,
    /// Documented total context window for display; `None` falls back to
    /// `max_context_tokens`. Purely cosmetic — all budget math uses
    /// `max_context_tokens`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advertised_context_window: Option<usize>,
    /// Tokens reserved for the system prompt
    pub system_prompt_tokens: usize,
    /// Output headroom reserved before compaction triggers. `None` uses
    /// [`crate::context::DEFAULT_RESERVE_TOKENS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve_tokens: Option<usize>,
    /// Explicit model-specific compaction threshold. `None` derives the
    /// threshold from `max_context_tokens - reserve_tokens`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_tokens: Option<usize>,
    /// Token budget for the retained tail. `None` uses
    /// [`crate::context::DEFAULT_KEEP_RECENT_TOKENS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_recent_tokens: Option<usize>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 100_000,
            advertised_context_window: None,
            system_prompt_tokens: 4_000,
            reserve_tokens: None,
            trigger_tokens: None,
            keep_recent_tokens: None,
        }
    }
}

impl ContextConfig {
    /// Derive context management from resolved model-profile metadata.
    pub fn from_model(
        model: &crate::provider::ModelConfig,
        thinking_level: crate::types::ThinkingLevel,
    ) -> Self {
        let max_context_tokens = model.context_window() as usize;
        Self {
            max_context_tokens,
            advertised_context_window: Some(model.advertised_context_window() as usize),
            trigger_tokens: model
                .profile_compaction_limit(thinking_level)
                .map(|limit| (limit as usize).min(max_context_tokens)),
            ..Default::default()
        }
    }

    /// Derive a context config from a model's context window size.
    ///
    /// Uses the supplied request-input limit as the budget. Output headroom is
    /// the sole responsibility of `CompactionConfig::reserve_tokens`, so the
    /// value is not pre-discounted here.
    pub fn from_context_window(context_window: u32) -> Self {
        Self {
            max_context_tokens: context_window as usize,
            ..Default::default()
        }
    }
}
