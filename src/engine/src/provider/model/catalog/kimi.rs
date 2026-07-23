use super::profile::ModelProfile;
use super::profile::BASE;
use crate::ThinkingLevel;
const K3_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::Minimal, None),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, None),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, None),
    (ThinkingLevel::Max, Some("max")),
];
const KIMI_CODING: ModelProfile = ModelProfile {
    context_window: 262_144,
    max_tokens: 32_768,
    adaptive_thinking: true,
    ..BASE
};

const K3: ModelProfile = ModelProfile {
    context_window: 1_048_576,
    max_tokens: 131_072,
    thinking_levels: K3_LEVELS,
    adaptive_thinking: true,
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("k2p7",                      KIMI_CODING),
    ("kimi-for-coding",           KIMI_CODING),
    ("kimi-for-coding-highspeed", KIMI_CODING),
    ("k3",      K3),
    ("kimi-k3", K3),
    ("kimi-k2-thinking", ModelProfile { context_window: 262_144, max_tokens: 32_768, vision: false, adaptive_thinking: true, ..BASE }),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}
