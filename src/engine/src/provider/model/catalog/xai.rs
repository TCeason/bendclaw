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
const GROK_4_6_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
];
const GROK_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GROK_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: None,
};
const GROK_4_6_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GROK_4_6_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: None,
};

const GROK_4_6: ModelProfile = ModelProfile {
    max_input_tokens: 500_000,
    max_output_tokens: 500_000,
    reasoning: GROK_4_6_REASONING,
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    // xAI's 500k model window; 200k is only the higher-price prompt tier.
    ("grok-4.5", ModelProfile { max_input_tokens: 500_000, max_output_tokens: 63_356, reasoning: GROK_REASONING, ..BASE }),
    ("grok-4.6", GROK_4_6),
    ("grok-composer-2.5-fast", ModelProfile { max_input_tokens: 200_000, max_output_tokens: 200_000, vision: false, reasoning: NO_REASONING, ..BASE }),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}

/// Uncatalogued Grok 4.5+ ids inherit the current 500k window.
pub(super) fn fallback(id: &str) -> Option<ModelProfile> {
    super::profile::version_at_least(id, "grok-", (4, 5)).then_some(GROK_4_6)
}
