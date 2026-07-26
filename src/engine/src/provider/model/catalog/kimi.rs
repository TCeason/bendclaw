use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use crate::ThinkingLevel;

const KIMI_CODING_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, Some("none")),
    (ThinkingLevel::High, Some("high")),
];
const K3_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, Some("none")),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Max, Some("max")),
];
const KIMI_CODING_REASONING: ReasoningProfile = ReasoningProfile {
    levels: KIMI_CODING_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: Some(super::super::capabilities::AnthropicThinkingWire::Enabled),
};
const K3_REASONING: ReasoningProfile = ReasoningProfile {
    levels: K3_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: Some(super::super::capabilities::AnthropicThinkingWire::Enabled),
};

const KIMI_CODING: ModelProfile = ModelProfile {
    max_input_tokens: 196_608,
    max_output_tokens: 65_536,
    reasoning: KIMI_CODING_REASONING,
    ..BASE
};
const K3: ModelProfile = ModelProfile {
    max_input_tokens: 196_608,
    max_output_tokens: 65_536,
    reasoning: K3_REASONING,
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("k2p7",                       KIMI_CODING),
    ("kimi-k2.7-code",             KIMI_CODING),
    ("kimi-for-coding",            KIMI_CODING),
    ("kimi-for-coding-highspeed",  KIMI_CODING),
    ("k3",                         K3),
    ("kimi-k3",                    K3),
    ("kimi-k2.6",                  KIMI_CODING),
    ("kimi-k2-thinking",           ModelProfile { vision: false, ..KIMI_CODING }),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}
