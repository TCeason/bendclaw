//! Model-level thinking / reasoning-effort helpers.
//!
//! Model metadata determines whether reasoning exists and which levels are
//! selectable. Transport metadata independently determines the wire format and
//! whether that endpoint can carry an effort value.

use super::model::ApiProtocol;
use super::model::CompatCaps;
use super::model::ModelConfig;
use super::model::ThinkingFormat;
use crate::ThinkingLevel;

/// Default token budgets for Anthropic budget-based thinking (non-adaptive).
pub const DEFAULT_BUDGET_MINIMAL: u32 = 1024;
pub const DEFAULT_BUDGET_LOW: u32 = 2048;
pub const DEFAULT_BUDGET_MEDIUM: u32 = 8192;
pub const DEFAULT_BUDGET_HIGH: u32 = 16_384;
/// Anthropic's minimum budget-based thinking allocation.
pub const MIN_THINKING_BUDGET: u32 = 1024;
/// Leave at least this many tokens for the visible answer.
pub const MIN_OUTPUT_AFTER_THINKING: u32 = 1024;

/// Ordered ladder used for selection UI and nearest-level clamping.
const LEVEL_LADDER: [ThinkingLevel; 8] = [
    ThinkingLevel::Off,
    ThinkingLevel::Minimal,
    ThinkingLevel::Low,
    ThinkingLevel::Medium,
    ThinkingLevel::High,
    ThinkingLevel::Xhigh,
    ThinkingLevel::Max,
    ThinkingLevel::Adaptive,
];

impl ModelConfig {
    /// Whether the configured protocol and transport can carry a selectable
    /// reasoning effort for this model.
    pub fn honors_reasoning_effort(&self) -> bool {
        match self.api {
            ApiProtocol::AnthropicMessages
            | ApiProtocol::OpenAiResponses
            | ApiProtocol::BedrockConverseStream => true,
            ApiProtocol::OpenAiCompletions => self.compat.as_ref().is_some_and(|compat| {
                let format_carries_effort = match compat.thinking_format {
                    ThinkingFormat::OpenRouter => true,
                    ThinkingFormat::DeepSeek => !self.thinking_level_map.is_empty(),
                    _ => false,
                };
                format_carries_effort || compat.caps.contains(CompatCaps::REASONING_EFFORT)
            }),
        }
    }

    pub fn supported_thinking_levels(&self) -> Vec<ThinkingLevel> {
        if !self.reasoning {
            return vec![ThinkingLevel::Off];
        }
        if self.api == ApiProtocol::OpenAiCompletions && !self.honors_reasoning_effort() {
            return Vec::new();
        }
        LEVEL_LADDER
            .into_iter()
            .filter(|level| {
                // Adaptive is an evot-only default alias, not a selectable ramp step.
                *level != ThinkingLevel::Adaptive && self.level_selectable(*level)
            })
            .collect()
    }

    fn level_selectable(&self, level: ThinkingLevel) -> bool {
        match self.thinking_level_map.get(level.as_str()) {
            Some(Some(effort)) => {
                // Hide alias-only xhigh when a real max tier is also present.
                !(level == ThinkingLevel::Xhigh
                    && effort == "max"
                    && self.thinking_level_map.contains_key("max"))
            }
            Some(None) => false,
            None => !matches!(level, ThinkingLevel::Xhigh | ThinkingLevel::Max),
        }
    }

    pub fn thinking_effort_override(&self, level: ThinkingLevel) -> Option<&str> {
        self.thinking_level_map.get(level.as_str())?.as_deref()
    }

    pub fn can_disable_thinking(&self) -> bool {
        !matches!(self.thinking_level_map.get("off"), Some(None))
    }

    /// Clamp a requested level to the nearest supported tier for this model.
    ///
    /// Walks the ladder upward first, then downward (mirrors pi's
    /// `clampThinkingLevel`). Falls back to the first supported level, or
    /// `Off` when the model has no selectable levels.
    pub fn clamp_thinking_level(&self, level: ThinkingLevel) -> ThinkingLevel {
        let available = self.supported_thinking_levels();
        if available.is_empty() {
            return ThinkingLevel::Off;
        }
        if available.contains(&level) {
            return level;
        }
        // Adaptive is not in the selectable ramp: treat it as High for clamping.
        let anchor = if level == ThinkingLevel::Adaptive {
            ThinkingLevel::High
        } else {
            level
        };
        let Some(idx) = LEVEL_LADDER.iter().position(|l| *l == anchor) else {
            return available[0];
        };
        for candidate in LEVEL_LADDER.iter().skip(idx) {
            if available.contains(candidate) {
                return *candidate;
            }
        }
        for candidate in LEVEL_LADDER.iter().take(idx).rev() {
            if available.contains(candidate) {
                return *candidate;
            }
        }
        available[0]
    }

    /// Resolve the level used for a request.
    ///
    /// Adaptive remains a model-selected alias only when the model has a
    /// selectable reasoning ramp. Every concrete level, including `Off`, is
    /// clamped so persisted settings from a previous model cannot bypass the
    /// current model's restrictions.
    pub fn effective_thinking_level(&self, requested: ThinkingLevel) -> ThinkingLevel {
        if !self.reasoning || self.supported_thinking_levels().is_empty() {
            return ThinkingLevel::Off;
        }
        if requested == ThinkingLevel::Adaptive {
            ThinkingLevel::Adaptive
        } else {
            self.clamp_thinking_level(requested)
        }
    }
}

/// Resolve a request level when model metadata may be absent.
pub fn effective_thinking_level(
    requested: ThinkingLevel,
    model: Option<&ModelConfig>,
) -> ThinkingLevel {
    model
        .map(|m| m.effective_thinking_level(requested))
        .unwrap_or(requested)
}

/// Map a thinking level to an Anthropic adaptive-thinking effort value.
///
/// Per-model overrides win. Adaptive defaults to `"high"` (pi's default branch).
pub fn anthropic_effort(level: ThinkingLevel, model: Option<&ModelConfig>) -> Option<String> {
    if let Some(mapped) = model.and_then(|mc| mc.thinking_effort_override(level)) {
        return Some(mapped.to_string());
    }
    match level {
        ThinkingLevel::Off => None,
        ThinkingLevel::Minimal | ThinkingLevel::Low => Some("low".into()),
        ThinkingLevel::Medium => Some("medium".into()),
        ThinkingLevel::High | ThinkingLevel::Adaptive => Some("high".into()),
        ThinkingLevel::Xhigh => Some("xhigh".into()),
        ThinkingLevel::Max => Some("max".into()),
    }
}

/// Token budget for Anthropic budget-based thinking.
pub fn anthropic_thinking_budget(level: ThinkingLevel) -> u32 {
    match level {
        ThinkingLevel::Off => 0,
        ThinkingLevel::Minimal => DEFAULT_BUDGET_MINIMAL,
        ThinkingLevel::Low => DEFAULT_BUDGET_LOW,
        ThinkingLevel::Medium | ThinkingLevel::Adaptive => DEFAULT_BUDGET_MEDIUM,
        ThinkingLevel::High | ThinkingLevel::Xhigh | ThinkingLevel::Max => DEFAULT_BUDGET_HIGH,
    }
}

/// Adjust max_tokens so budget-based thinking fits inside the model output cap.
///
/// Returns `(max_tokens, thinking_budget)` after fitting the budget and leaving
/// room for a visible answer (mirrors pi's `adjustMaxTokensForThinking`).
pub fn adjust_max_tokens_for_thinking(
    explicit_max_tokens: Option<u32>,
    model_max_tokens: u32,
    level: ThinkingLevel,
) -> (u32, u32) {
    let mut thinking_budget = anthropic_thinking_budget(level);
    let max_tokens = match explicit_max_tokens {
        Some(base) => base.saturating_add(thinking_budget).min(model_max_tokens),
        None => model_max_tokens,
    }
    .max(1);
    let room_for_answer = max_tokens.saturating_sub(MIN_OUTPUT_AFTER_THINKING);
    thinking_budget = thinking_budget.min(room_for_answer);
    (max_tokens, thinking_budget)
}

/// Whether `Off` should emit `{"type":"disabled"}` rather than omit the field.
pub fn thinking_off_disables(model: Option<&ModelConfig>) -> bool {
    model.map(ModelConfig::can_disable_thinking).unwrap_or(true)
}

/// Whether this model uses Anthropic adaptive thinking (`output_config.effort`).
pub fn force_adaptive_thinking(model: Option<&ModelConfig>) -> bool {
    model.map(|m| m.force_adaptive_thinking).unwrap_or(false)
}
