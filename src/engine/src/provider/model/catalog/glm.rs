use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use crate::ThinkingLevel;

const GLM_5_2_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, Some("none")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Max, Some("max")),
];
const GLM_5_2_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GLM_5_2_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: None,
};

const GLM_5_2: ModelProfile = ModelProfile {
    max_input_tokens: 908_928,
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
