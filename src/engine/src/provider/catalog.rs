//! Model capability catalog.
//!
//! Model metadata is keyed by model id, not by the configured transport channel.
//! A user may route `grok-4.5` through `openai` or `grok`; the context window and
//! reasoning map still come from this catalog.
//!
//! Layers:
//! 1. [`resolve`] — fixed per-model capabilities
//! 2. Provider transport (`OpenAiCompat`, base URL) — endpoint quirks
//! 3. Explicit env overrides — final user authority

use std::collections::HashMap;

use super::model::InputModality;

/// Intrinsic capabilities of a concrete model id.
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
    /// Whether the Anthropic model accepts a temperature parameter.
    pub supports_temperature: bool,
}

impl ModelMetadata {
    pub fn text_only(context_window: u32, max_tokens: u32) -> Self {
        Self {
            context_window,
            max_tokens,
            reasoning: true,
            input: vec![InputModality::Text],
            thinking_level_map: HashMap::new(),
            force_adaptive_thinking: false,
            supports_temperature: true,
        }
    }

    pub fn vision(context_window: u32, max_tokens: u32) -> Self {
        Self {
            context_window,
            max_tokens,
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            thinking_level_map: HashMap::new(),
            force_adaptive_thinking: false,
            supports_temperature: true,
        }
    }

    pub fn with_reasoning(mut self, reasoning: bool) -> Self {
        self.reasoning = reasoning;
        self
    }

    pub fn with_thinking_map(mut self, map: HashMap<String, Option<String>>) -> Self {
        self.thinking_level_map = map;
        self
    }

    pub fn with_adaptive_thinking(mut self, force: bool) -> Self {
        self.force_adaptive_thinking = force;
        self
    }

    pub fn with_temperature(mut self, supported: bool) -> Self {
        self.supports_temperature = supported;
        self
    }
}

/// Resolve metadata for a model id.
///
/// Accepts bare ids (`grok-4.5`) and common prefixed forms (`xai/grok-4.5`,
/// `openai/gpt-5.6-sol`). Returns `None` for unknown models so callers can apply
/// protocol-specific defaults.
pub fn resolve(model_id: &str) -> Option<ModelMetadata> {
    let id = normalize_model_id(model_id);
    if id.is_empty() {
        return None;
    }
    resolve_exact(&id)
        .or_else(|| resolve_openai_family(&id))
        .or_else(|| resolve_anthropic_family(&id))
        .or_else(|| resolve_grok_family(&id))
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
    let mut levels = HashMap::new();
    levels.insert("off".into(), None);
    levels.insert("minimal".into(), None);
    levels.insert("low".into(), Some("low".into()));
    levels.insert("medium".into(), None);
    levels.insert("high".into(), Some("high".into()));
    levels.insert("xhigh".into(), None);
    levels.insert("max".into(), Some("max".into()));
    levels
}

fn resolve_exact(id: &str) -> Option<ModelMetadata> {
    match id {
        // Kimi Coding model metadata mirrors pi's generated
        // `packages/ai/src/providers/kimi-coding.models.ts` catalog. These ids
        // use the Anthropic Messages transport but must not inherit the
        // conservative unknown-Anthropic fallback (200k context / 8k output).
        "k2p7" | "kimi-for-coding" | "kimi-for-coding-highspeed" => {
            Some(ModelMetadata::vision(262_144, 32_768).with_adaptive_thinking(true))
        }
        "k3" => Some(
            ModelMetadata::vision(1_048_576, 131_072)
                .with_thinking_map(kimi_k3_thinking_level_map())
                .with_adaptive_thinking(true),
        ),
        "kimi-k2-thinking" => {
            Some(ModelMetadata::text_only(262_144, 32_768).with_adaptive_thinking(true))
        }
        // Tiny fixture used by engine tests.
        #[cfg(test)]
        "tiny-context" => Some(ModelMetadata::text_only(128, 32_768)),
        _ => None,
    }
}
fn resolve_openai_family(id: &str) -> Option<ModelMetadata> {
    let (context_window, max_tokens) = match id {
        "gpt-5.4" | "gpt-5.5" | "gpt-5.6-luna" | "gpt-5.6-sol" | "gpt-5.6-terra" => {
            (272_000, 128_000)
        }
        "gpt-5.4-pro" | "gpt-5.5-pro" => (1_050_000, 128_000),
        // Other first-party OpenAI model ids: known family, conservative caps.
        _ if id.starts_with("gpt-")
            || id.starts_with("codex-")
            || id.starts_with("o1")
            || id.starts_with("o3")
            || id.starts_with("o4") =>
        {
            (128_000, 32_768)
        }
        _ => return None,
    };

    Some(
        ModelMetadata::vision(context_window, max_tokens)
            .with_thinking_map(openai_thinking_level_map(id)),
    )
}

fn openai_thinking_level_map(id: &str) -> HashMap<String, Option<String>> {
    let mut map = HashMap::new();
    if !(id.starts_with("gpt-5") || id.starts_with("codex-")) {
        return map;
    }

    let default = if id == "gpt-5.4" { "xhigh" } else { "medium" };
    map.insert("adaptive".into(), Some(default.into()));

    // xhigh only on gpt-5.2+ (pi's supportsOpenAiXhigh).
    if supports_openai_xhigh(id) {
        map.insert("xhigh".into(), Some("xhigh".into()));
    }

    // max only on gpt-5.6 family.
    if id.contains("gpt-5.6") {
        map.insert("max".into(), Some("max".into()));
    }

    if id.ends_with("gpt-5.5-pro") {
        map.insert("off".into(), None);
        map.insert("minimal".into(), None);
        map.insert("low".into(), None);
    }

    map
}

fn supports_openai_xhigh(id: &str) -> bool {
    id.contains("gpt-5.2")
        || id.contains("gpt-5.3")
        || id.contains("gpt-5.4")
        || id.contains("gpt-5.5")
        || id.contains("gpt-5.6")
}

fn resolve_anthropic_family(id: &str) -> Option<ModelMetadata> {
    let Some((family, major, minor)) = anthropic_model_version(id) else {
        // Known Anthropic id shape without a modern version gate.
        if id.contains("claude") || id.contains("fable") {
            return Some(ModelMetadata::vision(200_000, 8192).with_thinking_map(HashMap::new()));
        }
        return None;
    };

    let adaptive = is_anthropic_adaptive(family, major, minor);
    let million_ctx = adaptive;
    let (context_window, max_tokens) = if million_ctx {
        (1_000_000, 128_000)
    } else if major >= 4 {
        (200_000, 64_000)
    } else {
        (200_000, 8192)
    };

    let supports_temperature = !(family == "opus" && major == 4 && matches!(minor, 7 | 8));
    Some(
        ModelMetadata::vision(context_window, max_tokens)
            .with_thinking_map(anthropic_thinking_level_map(family, major, minor))
            .with_adaptive_thinking(adaptive)
            .with_temperature(supports_temperature),
    )
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

fn resolve_grok_family(id: &str) -> Option<ModelMetadata> {
    match id {
        "grok-4.5" => {
            let mut levels = HashMap::new();
            levels.insert("off".into(), None);
            levels.insert("minimal".into(), None);
            levels.insert("low".into(), Some("low".into()));
            levels.insert("medium".into(), Some("medium".into()));
            levels.insert("high".into(), Some("high".into()));
            levels.insert("adaptive".into(), Some("high".into()));
            levels.insert("xhigh".into(), None);
            levels.insert("max".into(), None);
            Some(ModelMetadata::vision(500_000, 500_000).with_thinking_map(levels))
        }
        "grok-composer-2.5-fast" => {
            Some(ModelMetadata::text_only(200_000, 200_000).with_reasoning(false))
        }
        _ => None,
    }
}
