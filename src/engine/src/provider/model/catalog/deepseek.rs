use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use super::profile::NO_REASONING;
use crate::ThinkingLevel;

const REASONER_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::High, Some("high")),
];
const REASONER: ReasoningProfile = ReasoningProfile {
    levels: REASONER_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: None,
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("deepseek-chat",     ModelProfile { max_input_tokens: 128_000, max_output_tokens: 8_192, vision: false, reasoning: NO_REASONING, ..BASE }),
    ("deepseek-reasoner", ModelProfile { max_input_tokens: 128_000, max_output_tokens: 64_000, vision: false, reasoning: REASONER, ..BASE }),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}
