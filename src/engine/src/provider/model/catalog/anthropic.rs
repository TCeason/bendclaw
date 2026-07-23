use super::profile::ModelProfile;
use super::profile::BASE;
use crate::ThinkingLevel;
const FABLE_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Max, Some("max")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
    (ThinkingLevel::Off, None),
];
const MAX_XHIGH_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Max, Some("max")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
];
const MAX_ONLY_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Max, Some("max")),
    (ThinkingLevel::Xhigh, Some("max")),
];

const ADAPTIVE_XHIGH: ModelProfile = ModelProfile {
    context_window: 1_000_000,
    max_tokens: 128_000,
    thinking_levels: MAX_XHIGH_LEVELS,
    adaptive_thinking: true,
    ..BASE
};
const ADAPTIVE_MAX_ONLY: ModelProfile = ModelProfile {
    context_window: 1_000_000,
    max_tokens: 128_000,
    thinking_levels: MAX_ONLY_LEVELS,
    adaptive_thinking: true,
    ..BASE
};
const MODERN: ModelProfile = ModelProfile {
    context_window: 200_000,
    max_tokens: 64_000,
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("claude-fable-5",    ModelProfile { context_window: 1_000_000, max_tokens: 128_000, thinking_levels: FABLE_LEVELS, adaptive_thinking: true, ..BASE }),
    ("claude-opus-4-8",   ADAPTIVE_XHIGH),
    ("claude-opus-4-7",   ADAPTIVE_XHIGH),
    ("claude-opus-4-6",   ADAPTIVE_MAX_ONLY),
    ("claude-opus-4-5",   MODERN),
    ("claude-opus-4-1",   MODERN),
    ("claude-sonnet-5",   ADAPTIVE_XHIGH),
    ("claude-sonnet-4-6", ADAPTIVE_MAX_ONLY),
    ("claude-sonnet-4-5", MODERN),
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

    let adaptive = is_adaptive(family, major, minor);
    let mut profile = if adaptive {
        ModelProfile {
            context_window: 1_000_000,
            max_tokens: 128_000,
            adaptive_thinking: true,
            ..BASE
        }
    } else if major >= 4 {
        MODERN
    } else {
        BASE
    };
    profile.thinking_levels = thinking_levels(family, major, minor);
    Some(profile)
}

fn is_adaptive(family: &str, major: u32, minor: u32) -> bool {
    family == "fable"
        || (family == "opus" && (major, minor) >= (4, 6))
        || (family == "sonnet" && ((major, minor) >= (4, 6) || major >= 5))
}

fn thinking_levels(
    family: &str,
    major: u32,
    minor: u32,
) -> &'static [(ThinkingLevel, Option<&'static str>)] {
    let adaptive = is_adaptive(family, major, minor);
    let supports_xhigh = family == "fable"
        || (family == "opus" && (major, minor) >= (4, 7))
        || (family == "sonnet" && major >= 5);
    if family == "fable" {
        FABLE_LEVELS
    } else if supports_xhigh {
        MAX_XHIGH_LEVELS
    } else if adaptive {
        MAX_ONLY_LEVELS
    } else {
        &[]
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
