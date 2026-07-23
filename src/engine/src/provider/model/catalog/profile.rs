use std::collections::HashMap;

use super::super::capabilities::InputModality;
use super::super::capabilities::ModelCapabilities;
use super::super::capabilities::ReasoningCapabilities;
use super::super::capabilities::Verbosity;
use crate::ThinkingLevel;

pub(super) type ThinkingLevels = &'static [(ThinkingLevel, Option<&'static str>)];

#[derive(Debug, Clone, Copy)]
pub(super) struct ModelProfile {
    pub context_window: u32,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub vision: bool,
    pub thinking_levels: ThinkingLevels,
    pub adaptive_thinking: bool,
    pub remote_compaction: bool,
    pub default_verbosity: Option<Verbosity>,
}

pub(super) const BASE: ModelProfile = ModelProfile {
    context_window: 200_000,
    max_tokens: 8_192,
    reasoning: true,
    vision: true,
    thinking_levels: &[],
    adaptive_thinking: false,
    remote_compaction: false,
    default_verbosity: None,
};

impl ModelProfile {
    pub(super) fn capabilities(self) -> ModelCapabilities {
        ModelCapabilities {
            context_window: self.context_window,
            max_output_tokens: self.max_tokens,
            input: if self.vision {
                vec![InputModality::Text, InputModality::Image]
            } else {
                vec![InputModality::Text]
            },
            reasoning: ReasoningCapabilities::new(
                self.reasoning,
                levels_map(self.thinking_levels),
                self.adaptive_thinking,
            ),
            default_verbosity: self.default_verbosity,
            remote_compaction: self.remote_compaction,
        }
    }
}

pub(super) fn levels_map(levels: ThinkingLevels) -> HashMap<ThinkingLevel, Option<String>> {
    levels
        .iter()
        .map(|(level, effort)| (*level, effort.map(str::to_string)))
        .collect()
}
