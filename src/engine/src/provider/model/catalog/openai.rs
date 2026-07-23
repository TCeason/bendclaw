use super::super::capabilities::Verbosity;
use super::profile::ModelProfile;
use super::profile::BASE;
use crate::ThinkingLevel;

const ADAPTIVE_LEVELS: &[(ThinkingLevel, Option<&str>)] =
    &[(ThinkingLevel::Adaptive, Some("medium"))];
const GPT_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Adaptive, Some("medium")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
];
const GPT_5_4_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Adaptive, Some("xhigh")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
];
const GPT_5_6_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Adaptive, Some("medium")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
    (ThinkingLevel::Max, Some("max")),
];
const GPT_5_5_PRO_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Adaptive, Some("medium")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
    (ThinkingLevel::Off, None),
    (ThinkingLevel::Minimal, None),
    (ThinkingLevel::Low, None),
];

const OFF_NONE: &[(ThinkingLevel, Option<&str>)] = &[(ThinkingLevel::Off, Some("none"))];
const GPT_5_5_FIRST_PARTY: &[(ThinkingLevel, Option<&str>)] = &[(ThinkingLevel::Minimal, None)];

const GPT_5_6: ModelProfile = ModelProfile {
    context_window: 272_000,
    max_tokens: 128_000,
    thinking_levels: GPT_5_6_LEVELS,
    first_party_responses_thinking_levels: OFF_NONE,
    remote_compaction: true,
    default_verbosity: Some(Verbosity::Low),
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("gpt-5.4",       ModelProfile { context_window: 272_000, max_tokens: 128_000, thinking_levels: GPT_5_4_LEVELS, first_party_responses_thinking_levels: OFF_NONE, remote_compaction: true, ..BASE }),
    ("gpt-5.4-pro",   ModelProfile { context_window: 1_050_000, max_tokens: 128_000, thinking_levels: GPT_LEVELS, remote_compaction: true, ..BASE }),
    ("gpt-5.5",       ModelProfile { context_window: 272_000, max_tokens: 128_000, thinking_levels: GPT_LEVELS, first_party_thinking_levels: GPT_5_5_FIRST_PARTY, first_party_responses_thinking_levels: OFF_NONE, remote_compaction: true, default_verbosity: Some(Verbosity::Low), ..BASE }),
    ("gpt-5.5-pro",   ModelProfile { context_window: 1_050_000, max_tokens: 128_000, thinking_levels: GPT_5_5_PRO_LEVELS, remote_compaction: true, ..BASE }),
    ("gpt-5.6-luna",  GPT_5_6),
    ("gpt-5.6-sol",   GPT_5_6),
    ("gpt-5.6-terra", GPT_5_6),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}

/// Conservative metadata for uncatalogued OpenAI families. Model-native
/// extensions remain opt-in, while reasoning defaults follow the family rules.
pub(super) fn fallback(id: &str) -> Option<ModelProfile> {
    if id.starts_with("gpt-") || id.starts_with("codex-") {
        let thinking_levels = if id.contains("gpt-5.6") {
            GPT_5_6_LEVELS
        } else if ["gpt-5.2", "gpt-5.3", "gpt-5.4", "gpt-5.5"]
            .iter()
            .any(|family| id.contains(family))
        {
            GPT_LEVELS
        } else if id.starts_with("gpt-5") || id.starts_with("codex-") {
            ADAPTIVE_LEVELS
        } else {
            &[]
        };
        return Some(ModelProfile {
            context_window: 128_000,
            max_tokens: 32_768,
            thinking_levels,
            first_party_responses_thinking_levels: first_party_levels(id),
            ..BASE
        });
    }
    if id.starts_with("o1") || id.starts_with("o3") || id.starts_with("o4") {
        return Some(ModelProfile {
            context_window: 128_000,
            max_tokens: 32_768,
            vision: false,
            ..BASE
        });
    }
    None
}

fn first_party_levels(id: &str) -> super::profile::ThinkingLevels {
    if matches!(
        id,
        "gpt-5.1" | "gpt-5.2" | "gpt-5.3-codex" | "gpt-5.4-mini" | "gpt-5.4-nano"
    ) {
        OFF_NONE
    } else {
        &[]
    }
}
