use super::super::capabilities::AnthropicThinkingWire;
use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use crate::ThinkingLevel;

const CLAUDE_STANDARD_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
];
const CLAUDE_MAX_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Max, Some("max")),
];
const CLAUDE_XHIGH_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
    (ThinkingLevel::Max, Some("max")),
];

const STANDARD_REASONING: ReasoningProfile = ReasoningProfile {
    levels: CLAUDE_STANDARD_LEVELS,
    default: ThinkingLevel::Off,
    anthropic_wire: None,
};
const ADAPTIVE_MAX_REASONING: ReasoningProfile = ReasoningProfile {
    levels: CLAUDE_MAX_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: Some(AnthropicThinkingWire::Adaptive),
};
const ADAPTIVE_XHIGH_REASONING: ReasoningProfile = ReasoningProfile {
    levels: CLAUDE_XHIGH_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: Some(AnthropicThinkingWire::Adaptive),
};
const FABLE_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, None),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
    (ThinkingLevel::Max, Some("max")),
];
const FABLE_REASONING: ReasoningProfile = ReasoningProfile {
    levels: FABLE_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: Some(AnthropicThinkingWire::Adaptive),
};

const MODERN: ModelProfile = ModelProfile {
    max_input_tokens: 200_000,
    max_output_tokens: 64_000,
    reasoning: STANDARD_REASONING,
    compaction_limit: Some(180_000),
    ..BASE
};
const OPUS_LONG_CONTEXT_XHIGH: ModelProfile = ModelProfile {
    max_input_tokens: 867_000,
    max_output_tokens: 128_000,
    reasoning: ADAPTIVE_XHIGH_REASONING,
    ..BASE
};
const SONNET_LONG_CONTEXT_XHIGH: ModelProfile = ModelProfile {
    max_input_tokens: 872_000,
    max_output_tokens: 128_000,
    reasoning: ADAPTIVE_XHIGH_REASONING,
    ..BASE
};
const FABLE: ModelProfile = ModelProfile {
    max_input_tokens: 867_000,
    max_output_tokens: 128_000,
    reasoning: FABLE_REASONING,
    ..BASE
};
const OPUS_LONG_CONTEXT_MAX: ModelProfile = ModelProfile {
    max_input_tokens: 867_000,
    max_output_tokens: 128_000,
    reasoning: ADAPTIVE_MAX_REASONING,
    ..BASE
};
const SONNET_LONG_CONTEXT_MAX: ModelProfile = ModelProfile {
    max_input_tokens: 931_000,
    max_output_tokens: 64_000,
    reasoning: ADAPTIVE_MAX_REASONING,
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("claude-fable-5",    FABLE),
    ("claude-opus-5",     OPUS_LONG_CONTEXT_XHIGH),
    ("claude-opus-4-8",   OPUS_LONG_CONTEXT_XHIGH),
    ("claude-opus-4-7",   OPUS_LONG_CONTEXT_XHIGH),
    ("claude-opus-4-6",   OPUS_LONG_CONTEXT_MAX),
    ("claude-opus-4-5",   MODERN),
    ("claude-sonnet-5",   SONNET_LONG_CONTEXT_XHIGH),
    ("claude-sonnet-4-6", SONNET_LONG_CONTEXT_MAX),
    ("claude-sonnet-4-5", MODERN),
    ("claude-sonnet-4",   MODERN),
    ("claude-haiku-4-5",  MODERN),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}

pub(super) fn fallback(id: &str) -> Option<ModelProfile> {
    let Some((family, major, minor)) = model_version(id) else {
        return (id.contains("claude") || id.contains("fable")).then_some(BASE);
    };

    if family == "fable"
        || (family == "opus" && (major, minor) >= (4, 7))
        || (family == "sonnet" && major >= 5)
    {
        Some(if family == "fable" {
            FABLE
        } else if family == "sonnet" {
            SONNET_LONG_CONTEXT_XHIGH
        } else {
            OPUS_LONG_CONTEXT_XHIGH
        })
    } else if family == "opus" && (major, minor) >= (4, 6) {
        Some(OPUS_LONG_CONTEXT_MAX)
    } else if family == "sonnet" && (major, minor) >= (4, 6) {
        Some(SONNET_LONG_CONTEXT_MAX)
    } else if major >= 4 {
        Some(MODERN)
    } else {
        Some(BASE)
    }
}

fn model_version(id: &str) -> Option<(&'static str, u32, u32)> {
    let family = ["opus", "sonnet", "haiku", "fable"]
        .into_iter()
        .find(|family| id.contains(*family))?;
    let after = id.split(family).nth(1)?;
    let mut parts = after
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| (1..=2).contains(&part.len()));
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    Some((family, major, minor))
}
