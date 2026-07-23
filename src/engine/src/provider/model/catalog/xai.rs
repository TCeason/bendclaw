use super::profile::ModelProfile;
use super::profile::BASE;
use crate::ThinkingLevel;
const GROK_4_5_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::Minimal, None),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Adaptive, Some("high")),
    (ThinkingLevel::Xhigh, None),
    (ThinkingLevel::Max, None),
];

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("grok-4.5", ModelProfile { context_window: 500_000, max_tokens: 500_000, thinking_levels: GROK_4_5_LEVELS, ..BASE }),
    ("grok-composer-2.5-fast", ModelProfile { context_window: 200_000, max_tokens: 200_000, reasoning: false, vision: false, ..BASE }),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}
