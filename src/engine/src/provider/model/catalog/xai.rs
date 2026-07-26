use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use super::profile::NO_REASONING;
use crate::ThinkingLevel;

const GROK_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
];
const GROK_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GROK_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: None,
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    // xAI's 500k model window; 200k is only the higher-price prompt tier.
    ("grok-4.5", ModelProfile { max_input_tokens: 500_000, max_output_tokens: 63_356, reasoning: GROK_REASONING, ..BASE }),
    ("grok-composer-2.5-fast", ModelProfile { max_input_tokens: 200_000, max_output_tokens: 200_000, vision: false, reasoning: NO_REASONING, ..BASE }),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}
