use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use crate::ThinkingLevel;

/// A modality accepted by a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputModality {
    Text,
    Image,
}

/// Native model control for final-answer length and detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Verbosity {
    Low,
    #[default]
    Medium,
    High,
}

/// Wire encoding for effort-based thinking on the Anthropic protocol.
///
/// Anthropic-compatible endpoints do not all speak the same dialect: Claude
/// accepts the proprietary `{"type":"adaptive"}` extension, while
/// compatible third-party endpoints (e.g. Kimi) only accept
/// `{"type":"enabled"}` and silently ignore unknown types — which would
/// disable thinking entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnthropicThinkingWire {
    /// Claude: `{"type":"adaptive","display":"summarized"}` + `output_config.effort`.
    Adaptive,
    /// Compatible endpoints (Kimi): `{"type":"enabled"}` + `output_config.effort`.
    Enabled,
}

/// Effective policy for one thinking level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThinkingLevelPolicy<'a> {
    ProtocolDefault,
    Unsupported,
    WireValue(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EffortMapping {
    Unsupported,
    Value(String),
}

/// Model-level reasoning support and wire-value overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ReasoningCapabilities {
    supported: bool,
    level_map: HashMap<ThinkingLevel, EffortMapping>,
    /// Effort-based thinking wire encoding; `None` means budget-based.
    effort_wire: Option<AnthropicThinkingWire>,
}

impl ReasoningCapabilities {
    pub(super) fn new(
        supported: bool,
        level_map: HashMap<ThinkingLevel, Option<String>>,
        effort_wire: Option<AnthropicThinkingWire>,
    ) -> Self {
        Self {
            supported,
            level_map: level_map
                .into_iter()
                .map(|(level, value)| {
                    let mapping = match value {
                        Some(value) => EffortMapping::Value(value),
                        None => EffortMapping::Unsupported,
                    };
                    (level, mapping)
                })
                .collect(),
            effort_wire,
        }
    }

    pub(super) fn supported(&self) -> bool {
        self.supported
    }

    pub(super) fn policy(&self, level: ThinkingLevel) -> ThinkingLevelPolicy<'_> {
        match self.level_map.get(&level) {
            Some(EffortMapping::Unsupported) => ThinkingLevelPolicy::Unsupported,
            Some(EffortMapping::Value(value)) => ThinkingLevelPolicy::WireValue(value),
            None => ThinkingLevelPolicy::ProtocolDefault,
        }
    }

    pub(super) fn has_wire_value(&self, value: &str) -> bool {
        self.level_map
            .values()
            .any(|mapping| matches!(mapping, EffortMapping::Value(mapped) if mapped == value))
    }

    pub(super) fn insert_override(&mut self, level: ThinkingLevel, value: Option<String>) {
        let mapping = match value {
            Some(value) => EffortMapping::Value(value),
            None => EffortMapping::Unsupported,
        };
        self.level_map.insert(level, mapping);
    }

    pub(super) fn set_supported(&mut self, supported: bool) {
        self.supported = supported;
    }

    pub(super) fn effort_wire(&self) -> Option<AnthropicThinkingWire> {
        self.effort_wire
    }
}

/// Intrinsic capabilities resolved from the model catalog.
#[derive(Debug, Clone)]
pub(super) struct ModelCapabilities {
    pub(super) context_window: u32,
    pub(super) max_output_tokens: u32,
    pub(super) input: Vec<InputModality>,
    pub(super) reasoning: ReasoningCapabilities,
    pub(super) first_party_reasoning_levels: HashMap<ThinkingLevel, Option<String>>,
    pub(super) first_party_responses_reasoning_levels: HashMap<ThinkingLevel, Option<String>>,
    pub(super) default_verbosity: Option<Verbosity>,
    pub(super) remote_compaction: bool,
}

impl ModelCapabilities {
    pub(super) fn supports_image(&self) -> bool {
        self.input.contains(&InputModality::Image)
    }
}
