use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use crate::ThinkingLevel;

// GLM-5.2 exposes two thinking-effort tiers (docs.z.ai): "high" and "max".
// The top tier is Evot's Xhigh level; ThinkingLevel::Max is reserved for models
// exposing both tiers, so binding "max" there would strand Xhigh users on High.
const GLM_5_2_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, Some("none")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("max")),
];
// GLM-5.2's Anthropic-compatible endpoint accepts `output_config.effort`
// alongside `thinking.type=enabled` — same dialect as Kimi.
const GLM_5_2_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GLM_5_2_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: Some(super::super::capabilities::AnthropicThinkingWire::Enabled),
};

// 1M total window (docs.z.ai) minus the 131_072 output budget.
const GLM_5_2: ModelProfile = ModelProfile {
    max_input_tokens: 917_504,
    advertised_context_window: Some(1_000_000),
    max_output_tokens: 131_072,
    vision: false,
    reasoning: GLM_5_2_REASONING,
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("glm-5.2",      GLM_5_2),
    ("glm-5.2-fast", GLM_5_2),
    ("glm-5p2",      GLM_5_2),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}

/// Any uncatalogued GLM id inherits the current 1M series window. Explicit
/// catalog entries above still win; later per-model configs override this.
pub(super) fn fallback(id: &str) -> Option<ModelProfile> {
    id.starts_with("glm").then_some(GLM_5_2)
}
