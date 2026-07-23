//! Model capability catalog.
//!
//! Known model ids are declarative [`ModelProfile`] records — one entry per
//! model in [`MODEL_PROFILES`], mirroring pi's generated model tables
//! (`packages/ai/src/providers/*.models.ts`). Capabilities are plain data;
//! resolution is a table lookup.
//!
//! Family fallback rules cover only ids absent from the table (date-suffixed
//! or not-yet-catalogued versions), so an unknown `gpt-*` or `claude-*` id
//! still gets sane limits.
//!
//! Metadata is keyed by model id, not by the configured transport channel.
//! A user may route `grok-4.5` through `openai` or `grok`; the context window
//! and reasoning map still come from this catalog.
//!
//! Layers:
//! 1. [`resolve`] — fixed per-model capabilities
//! 2. Provider transport (`OpenAiCompat`, base URL) — endpoint quirks
//! 3. Explicit env overrides — final user authority

use std::collections::HashMap;

use super::model::InputModality;

// ---------------------------------------------------------------------------
// Profile vocabulary
// ---------------------------------------------------------------------------

/// pi thinking level → wire effort. `None` marks the level unsupported.
type ThinkingLevels = &'static [(&'static str, Option<&'static str>)];

/// Declarative capability record for one model id.
#[derive(Debug, Clone, Copy)]
pub struct ModelProfile {
    pub context_window: u32,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub vision: bool,
    pub thinking_levels: ThinkingLevels,
    /// Anthropic adaptive thinking (`thinking.type: adaptive` +
    /// `output_config.effort`). When false, budget-based thinking is used for
    /// Anthropic transports.
    pub adaptive_thinking: bool,
    /// Server-side compaction via a `compaction_trigger` input item on the
    /// Responses API (GPT/Codex first-party upstreams).
    pub remote_compaction: bool,
}

/// Baseline every profile entry extends: vision reasoning model with
/// conservative legacy-Anthropic limits and no protocol extensions.
const BASE: ModelProfile = ModelProfile {
    context_window: 200_000,
    max_tokens: 8_192,
    reasoning: true,
    vision: true,
    thinking_levels: &[],
    adaptive_thinking: false,
    remote_compaction: false,
};

impl ModelProfile {
    fn metadata(&self) -> ModelMetadata {
        ModelMetadata {
            context_window: self.context_window,
            max_tokens: self.max_tokens,
            reasoning: self.reasoning,
            input: if self.vision {
                vec![InputModality::Text, InputModality::Image]
            } else {
                vec![InputModality::Text]
            },
            thinking_level_map: levels_map(self.thinking_levels),
            force_adaptive_thinking: self.adaptive_thinking,
            supports_remote_compaction: self.remote_compaction,
        }
    }
}

fn levels_map(levels: ThinkingLevels) -> HashMap<String, Option<String>> {
    levels
        .iter()
        .map(|(level, effort)| ((*level).to_string(), effort.map(str::to_string)))
        .collect()
}

// ---------------------------------------------------------------------------
// Thinking ramps shared by profile entries
// ---------------------------------------------------------------------------

const GPT_LEVELS: ThinkingLevels = &[("adaptive", Some("medium")), ("xhigh", Some("xhigh"))];
const GPT_5_4_LEVELS: ThinkingLevels = &[("adaptive", Some("xhigh")), ("xhigh", Some("xhigh"))];
const GPT_5_6_LEVELS: ThinkingLevels = &[
    ("adaptive", Some("medium")),
    ("xhigh", Some("xhigh")),
    ("max", Some("max")),
];
const GPT_5_5_PRO_LEVELS: ThinkingLevels = &[
    ("adaptive", Some("medium")),
    ("xhigh", Some("xhigh")),
    ("off", None),
    ("minimal", None),
    ("low", None),
];
const K3_LEVELS: ThinkingLevels = &[
    ("off", None),
    ("minimal", None),
    ("low", Some("low")),
    ("medium", None),
    ("high", Some("high")),
    ("xhigh", None),
    ("max", Some("max")),
];
const GROK_4_5_LEVELS: ThinkingLevels = &[
    ("off", None),
    ("minimal", None),
    ("low", Some("low")),
    ("medium", Some("medium")),
    ("high", Some("high")),
    ("adaptive", Some("high")),
    ("xhigh", None),
    ("max", None),
];
const FABLE_LEVELS: ThinkingLevels = &[
    ("max", Some("max")),
    ("xhigh", Some("xhigh")),
    ("off", None),
];
const ANTHROPIC_MAX_XHIGH_LEVELS: ThinkingLevels =
    &[("max", Some("max")), ("xhigh", Some("xhigh"))];
/// Adaptive models predating the xhigh tier: alias persisted `xhigh` to `max`.
const ANTHROPIC_MAX_ONLY_LEVELS: ThinkingLevels = &[("max", Some("max")), ("xhigh", Some("max"))];

// ---------------------------------------------------------------------------
// The catalog — one declarative record per known model id
// ---------------------------------------------------------------------------

const GPT_5_6: ModelProfile = ModelProfile {
    context_window: 272_000,
    max_tokens: 128_000,
    thinking_levels: GPT_5_6_LEVELS,
    remote_compaction: true,
    ..BASE
};

/// Anthropic adaptive-thinking generation (opus 4.7+, sonnet 5+).
const ANTHROPIC_ADAPTIVE_XHIGH: ModelProfile = ModelProfile {
    context_window: 1_000_000,
    max_tokens: 128_000,
    thinking_levels: ANTHROPIC_MAX_XHIGH_LEVELS,
    adaptive_thinking: true,
    ..BASE
};

/// First adaptive generation (opus 4.6, sonnet 4.6): max tier, no xhigh.
const ANTHROPIC_ADAPTIVE_MAX_ONLY: ModelProfile = ModelProfile {
    context_window: 1_000_000,
    max_tokens: 128_000,
    thinking_levels: ANTHROPIC_MAX_ONLY_LEVELS,
    adaptive_thinking: true,
    ..BASE
};

/// Budget-thinking Anthropic 4.x generation.
const ANTHROPIC_MODERN: ModelProfile = ModelProfile {
    context_window: 200_000,
    max_tokens: 64_000,
    ..BASE
};

/// Kimi Coding ids use the Anthropic Messages transport but must not inherit
/// the conservative unknown-Anthropic fallback. Mirrors pi's generated
/// `kimi-coding.models.ts`.
const KIMI_CODING: ModelProfile = ModelProfile {
    context_window: 262_144,
    max_tokens: 32_768,
    adaptive_thinking: true,
    ..BASE
};

#[rustfmt::skip]
const MODEL_PROFILES: &[(&str, ModelProfile)] = &[
    // -- OpenAI first-party (Responses transport, native compaction) --------
    ("gpt-5.4",       ModelProfile { context_window: 272_000, max_tokens: 128_000, thinking_levels: GPT_5_4_LEVELS, remote_compaction: true, ..BASE }),
    ("gpt-5.4-pro",   ModelProfile { context_window: 1_050_000, max_tokens: 128_000, thinking_levels: GPT_LEVELS, remote_compaction: true, ..BASE }),
    ("gpt-5.5",       ModelProfile { context_window: 272_000, max_tokens: 128_000, thinking_levels: GPT_LEVELS, remote_compaction: true, ..BASE }),
    ("gpt-5.5-pro",   ModelProfile { context_window: 1_050_000, max_tokens: 128_000, thinking_levels: GPT_5_5_PRO_LEVELS, remote_compaction: true, ..BASE }),
    ("gpt-5.6-luna",  GPT_5_6),
    ("gpt-5.6-sol",   GPT_5_6),
    ("gpt-5.6-terra", GPT_5_6),
    // -- Anthropic (date-suffixed ids resolve via the family fallback) --------
    ("claude-fable-5",    ModelProfile { context_window: 1_000_000, max_tokens: 128_000, thinking_levels: FABLE_LEVELS, adaptive_thinking: true, ..BASE }),
    ("claude-opus-4-8",   ANTHROPIC_ADAPTIVE_XHIGH),
    ("claude-opus-4-7",   ANTHROPIC_ADAPTIVE_XHIGH),
    ("claude-opus-4-6",   ANTHROPIC_ADAPTIVE_MAX_ONLY),
    ("claude-opus-4-5",   ANTHROPIC_MODERN),
    ("claude-opus-4-1",   ANTHROPIC_MODERN),
    ("claude-sonnet-5",   ANTHROPIC_ADAPTIVE_XHIGH),
    ("claude-sonnet-4-6", ANTHROPIC_ADAPTIVE_MAX_ONLY),
    ("claude-sonnet-4-5", ANTHROPIC_MODERN),
    ("claude-haiku-4-5",  ANTHROPIC_MODERN),
    // -- xAI -----------------------------------------------------------------
    ("grok-4.5",               ModelProfile { context_window: 500_000, max_tokens: 500_000, thinking_levels: GROK_4_5_LEVELS, ..BASE }),
    ("grok-composer-2.5-fast", ModelProfile { context_window: 200_000, max_tokens: 200_000, reasoning: false, vision: false, ..BASE }),
    // -- Kimi Coding (Anthropic Messages transport) ---------------------------
    ("k2p7",                      KIMI_CODING),
    ("kimi-for-coding",           KIMI_CODING),
    ("kimi-for-coding-highspeed", KIMI_CODING),
    ("k3",                ModelProfile { context_window: 1_048_576, max_tokens: 131_072, thinking_levels: K3_LEVELS, adaptive_thinking: true, ..BASE }),
    ("kimi-k2-thinking",  ModelProfile { context_window: 262_144, max_tokens: 32_768, vision: false, adaptive_thinking: true, ..BASE }),
];

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Intrinsic capabilities of a concrete model id, resolved from the catalog.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    pub context_window: u32,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub input: Vec<InputModality>,
    pub thinking_level_map: HashMap<String, Option<String>>,
    /// Anthropic adaptive thinking (`thinking.type: adaptive` + `output_config.effort`).
    /// When false, budget-based thinking is used for Anthropic transports.
    pub force_adaptive_thinking: bool,
    /// Server-side (provider-native) compaction via a `compaction_trigger`
    /// input item on the Responses API. GPT/Codex first-party models only.
    pub supports_remote_compaction: bool,
}

impl ModelMetadata {
    pub fn text_only(context_window: u32, max_tokens: u32) -> Self {
        ModelProfile {
            context_window,
            max_tokens,
            vision: false,
            ..BASE
        }
        .metadata()
    }

    pub fn vision(context_window: u32, max_tokens: u32) -> Self {
        ModelProfile {
            context_window,
            max_tokens,
            ..BASE
        }
        .metadata()
    }
}

/// Resolve metadata for a model id.
///
/// Accepts bare ids (`grok-4.5`) and common prefixed forms (`xai/grok-4.5`,
/// `openai/gpt-5.6-sol`). Returns `None` for unknown models so callers can
/// apply protocol-specific defaults.
pub fn resolve(model_id: &str) -> Option<ModelMetadata> {
    let id = normalize_model_id(model_id);
    if id.is_empty() {
        return None;
    }
    if let Some((_, profile)) = MODEL_PROFILES.iter().find(|(key, _)| *key == id) {
        return Some(profile.metadata());
    }
    openai_family_fallback(&id).or_else(|| anthropic_family_fallback(&id))
}

/// Normalize a model id for catalog lookup without changing the wire id.
pub fn normalize_model_id(model_id: &str) -> String {
    let normalized = model_id.trim().to_ascii_lowercase();
    for prefix in ["openai/", "xai/", "x-ai/", "anthropic/"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    normalized
}

pub(crate) fn kimi_k3_thinking_level_map() -> HashMap<String, Option<String>> {
    levels_map(K3_LEVELS)
}

// ---------------------------------------------------------------------------
// Family fallbacks — rules for ids missing from MODEL_PROFILES
// ---------------------------------------------------------------------------

/// First-party OpenAI ids not in the table: known family, conservative caps.
fn openai_family_fallback(id: &str) -> Option<ModelMetadata> {
    let profile = if id.starts_with("gpt-") || id.starts_with("codex-") {
        ModelProfile {
            context_window: 128_000,
            max_tokens: 32_768,
            remote_compaction: true,
            ..BASE
        }
    } else if id.starts_with("o1") || id.starts_with("o3") || id.starts_with("o4") {
        // o-series upstreams do not accept the `compaction_trigger` extension.
        ModelProfile {
            context_window: 128_000,
            max_tokens: 32_768,
            ..BASE
        }
    } else {
        return None;
    };
    let mut metadata = profile.metadata();
    metadata.thinking_level_map = openai_fallback_thinking_map(id);
    Some(metadata)
}

/// Version-rule thinking ramp for uncatalogued GPT/Codex ids, matching the
/// explicit ramps in [`MODEL_PROFILES`].
fn openai_fallback_thinking_map(id: &str) -> HashMap<String, Option<String>> {
    let mut map = HashMap::new();
    if !(id.starts_with("gpt-5") || id.starts_with("codex-")) {
        return map;
    }

    map.insert("adaptive".into(), Some("medium".into()));

    // xhigh only on gpt-5.2+ (pi's supportsOpenAiXhigh).
    let supports_xhigh = ["gpt-5.2", "gpt-5.3", "gpt-5.4", "gpt-5.5", "gpt-5.6"]
        .iter()
        .any(|family| id.contains(family));
    if supports_xhigh {
        map.insert("xhigh".into(), Some("xhigh".into()));
    }

    // max only on the gpt-5.6 family.
    if id.contains("gpt-5.6") {
        map.insert("max".into(), Some("max".into()));
    }

    map
}

/// Anthropic ids not in the table: capabilities are gated on the version
/// embedded in the id (which may carry a date suffix), so this stays a rule.
fn anthropic_family_fallback(id: &str) -> Option<ModelMetadata> {
    let Some((family, major, minor)) = anthropic_model_version(id) else {
        // Known Anthropic id shape without a modern version gate.
        if id.contains("claude") || id.contains("fable") {
            return Some(BASE.metadata());
        }
        return None;
    };

    let adaptive = is_anthropic_adaptive(family, major, minor);
    let profile = if adaptive {
        ModelProfile {
            context_window: 1_000_000,
            max_tokens: 128_000,
            adaptive_thinking: true,
            ..BASE
        }
    } else if major >= 4 {
        ModelProfile {
            context_window: 200_000,
            max_tokens: 64_000,
            ..BASE
        }
    } else {
        BASE
    };
    let mut metadata = profile.metadata();
    metadata.thinking_level_map = anthropic_thinking_level_map(family, major, minor);
    Some(metadata)
}

fn is_anthropic_adaptive(family: &str, major: u32, minor: u32) -> bool {
    family == "fable"
        || (family == "opus" && (major, minor) >= (4, 6))
        || (family == "sonnet" && ((major, minor) >= (4, 6) || major >= 5))
}

fn anthropic_thinking_level_map(
    family: &str,
    major: u32,
    minor: u32,
) -> HashMap<String, Option<String>> {
    let mut map = HashMap::new();
    let adaptive = is_anthropic_adaptive(family, major, minor);
    if adaptive {
        map.insert("max".into(), Some("max".into()));
    }
    let supports_xhigh = family == "fable"
        || (family == "opus" && (major, minor) >= (4, 7))
        || (family == "sonnet" && major >= 5);
    if supports_xhigh {
        map.insert("xhigh".into(), Some("xhigh".into()));
    } else if adaptive {
        // Keep old persisted `xhigh` settings valid on max-only models.
        map.insert("xhigh".into(), Some("max".into()));
    }
    if family == "fable" {
        map.insert("off".into(), None);
    }
    map
}

fn anthropic_model_version(id: &str) -> Option<(&'static str, u32, u32)> {
    let family = ["opus", "sonnet", "haiku", "fable"]
        .into_iter()
        .find(|f| id.contains(*f))?;
    let after = id.split(family).nth(1)?;
    let mut parts = after
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| (1..=2).contains(&s.len()));
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some((family, major, minor))
}
