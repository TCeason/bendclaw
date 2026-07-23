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
    /// Anthropic adaptive thinking (`thinking.type: adaptive`).
    force_adaptive: bool,
}

impl ReasoningCapabilities {
    pub(super) fn new(
        supported: bool,
        level_map: HashMap<ThinkingLevel, Option<String>>,
        force_adaptive: bool,
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
            force_adaptive,
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

    pub(super) fn replace_level_map(&mut self, map: HashMap<ThinkingLevel, Option<String>>) {
        *self = Self::new(self.supported, map, self.force_adaptive);
    }

    pub(super) fn set_supported(&mut self, supported: bool) {
        self.supported = supported;
    }

    pub(super) fn force_adaptive(&self) -> bool {
        self.force_adaptive
    }
}

/// Intrinsic capabilities resolved from the model catalog.
#[derive(Debug, Clone)]
pub(super) struct ModelCapabilities {
    pub(super) context_window: u32,
    pub(super) max_output_tokens: u32,
    pub(super) input: Vec<InputModality>,
    pub(super) reasoning: ReasoningCapabilities,
    pub(super) default_verbosity: Option<Verbosity>,
    pub(super) remote_compaction: bool,
}

impl ModelCapabilities {
    pub(super) fn supports_image(&self) -> bool {
        self.input.contains(&InputModality::Image)
    }
}
