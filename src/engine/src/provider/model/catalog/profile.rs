use std::collections::HashMap;

use super::super::capabilities::AnthropicThinkingWire;
use super::super::capabilities::InputModality;
use super::super::capabilities::ModelCapabilities;
use super::super::capabilities::ReasoningCapabilities;
use super::super::capabilities::Verbosity;
use crate::ThinkingLevel;

pub(super) type ReasoningLevels = &'static [(ThinkingLevel, Option<&'static str>)];

/// Intrinsic reasoning contract for a model. The list is exhaustive: a level
/// not present here is unsupported. This mirrors Droid's
/// `supportedReasoningEfforts` + `defaultReasoningEffort` model.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReasoningProfile {
    pub levels: ReasoningLevels,
    pub default: ThinkingLevel,
    /// Anthropic effort-based wire dialect. `None` selects budget thinking.
    pub anthropic_wire: Option<AnthropicThinkingWire>,
}

pub(super) const STANDARD_REASONING: ReasoningProfile = ReasoningProfile {
    levels: &[
        (ThinkingLevel::Off, None),
        (ThinkingLevel::Minimal, None),
        (ThinkingLevel::Low, None),
        (ThinkingLevel::Medium, None),
        (ThinkingLevel::High, None),
    ],
    default: ThinkingLevel::Medium,
    anthropic_wire: None,
};

pub(super) const NO_REASONING: ReasoningProfile = ReasoningProfile {
    levels: &[(ThinkingLevel::Off, None)],
    default: ThinkingLevel::Off,
    anthropic_wire: None,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct ModelProfile {
    /// Maximum request input accepted by the model, excluding generated output.
    pub max_input_tokens: u32,
    /// Maximum generated output accepted by the model.
    pub max_output_tokens: u32,
    pub vision: bool,
    pub reasoning: ReasoningProfile,
    pub remote_compaction: bool,
    /// Optional proactive compaction threshold. `None` derives the registry
    /// default from effective input capacity.
    pub compaction_limit: Option<u32>,
    pub default_verbosity: Option<Verbosity>,
}

pub(super) const BASE: ModelProfile = ModelProfile {
    max_input_tokens: 200_000,
    max_output_tokens: 8_192,
    vision: true,
    reasoning: STANDARD_REASONING,
    remote_compaction: false,
    compaction_limit: None,
    default_verbosity: None,
};

impl ModelProfile {
    pub(super) fn capabilities(self) -> ModelCapabilities {
        let compaction_limit = self
            .compaction_limit
            .unwrap_or(self.max_input_tokens.min(250_000));
        ModelCapabilities {
            max_input_tokens: self.max_input_tokens,
            max_output_tokens: self.max_output_tokens,
            input: if self.vision {
                vec![InputModality::Text, InputModality::Image]
            } else {
                vec![InputModality::Text]
            },
            reasoning: ReasoningCapabilities::new(
                levels_map(self.reasoning.levels),
                self.reasoning.default,
                self.reasoning.anthropic_wire,
            ),
            default_verbosity: self.default_verbosity,
            compaction_limit: Some(compaction_limit),
            remote_compaction: self.remote_compaction,
        }
    }
}

pub(super) fn levels_map(levels: ReasoningLevels) -> HashMap<ThinkingLevel, Option<String>> {
    levels
        .iter()
        .map(|(level, effort)| (*level, effort.map(str::to_string)))
        .collect()
}
