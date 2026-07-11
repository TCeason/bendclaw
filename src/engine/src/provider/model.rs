//! Model configuration and provider compatibility flags.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use crate::ThinkingLevel;

/// Which API protocol a model uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProtocol {
    AnthropicMessages,
    OpenAiCompletions,
    BedrockConverseStream,
}

impl std::fmt::Display for ApiProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnthropicMessages => write!(f, "anthropic_messages"),
            Self::OpenAiCompletions => write!(f, "openai_completions"),
            Self::BedrockConverseStream => write!(f, "bedrock_converse_stream"),
        }
    }
}

/// A modality a model accepts as input. Mirrors pi's `Model.input` array
/// (`("text" | "image")[]`), which the request builders consult before
/// attaching non-text content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputModality {
    Text,
    Image,
}

/// Cost per million tokens (input/output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostConfig {
    pub input_per_million: f64,
    pub output_per_million: f64,
    #[serde(default)]
    pub cache_read_per_million: f64,
    #[serde(default)]
    pub cache_write_per_million: f64,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            input_per_million: 0.0,
            output_per_million: 0.0,
            cache_read_per_million: 0.0,
            cache_write_per_million: 0.0,
        }
    }
}

/// How a provider handles the `max_tokens` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaxTokensField {
    #[default]
    MaxTokens,
    MaxCompletionTokens,
}

/// How a provider formats thinking/reasoning output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingFormat {
    #[default]
    OpenAi,
    Xai,
    Qwen,
}

/// Bitflag set of OpenAI-compatible provider capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompatCaps(u16);

impl CompatCaps {
    pub const NONE: Self = Self(0);
    pub const STORE: Self = Self(1 << 0);
    pub const DEVELOPER_ROLE: Self = Self(1 << 1);
    pub const REASONING_EFFORT: Self = Self(1 << 2);
    pub const USAGE_IN_STREAMING: Self = Self(1 << 3);
    pub const TOOL_RESULT_NAME: Self = Self(1 << 4);
    pub const ASSISTANT_AFTER_TOOL_RESULT: Self = Self(1 << 5);
    pub const REASONING_CONTENT_REQUIRED: Self = Self(1 << 6);
    pub const PROMPT_CACHE_KEY: Self = Self(1 << 7);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for CompatCaps {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for CompatCaps {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl Serialize for CompatCaps {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let all = [
            (Self::STORE, "store"),
            (Self::DEVELOPER_ROLE, "developer_role"),
            (Self::REASONING_EFFORT, "reasoning_effort"),
            (Self::USAGE_IN_STREAMING, "usage_in_streaming"),
            (Self::TOOL_RESULT_NAME, "tool_result_name"),
            (
                Self::ASSISTANT_AFTER_TOOL_RESULT,
                "assistant_after_tool_result",
            ),
            (
                Self::REASONING_CONTENT_REQUIRED,
                "reasoning_content_required",
            ),
            (Self::PROMPT_CACHE_KEY, "prompt_cache_key"),
        ];
        let count = all.iter().filter(|(f, _)| self.contains(*f)).count();
        let mut seq = serializer.serialize_seq(Some(count))?;
        for (flag, name) in &all {
            if self.contains(*flag) {
                seq.serialize_element(name)?;
            }
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for CompatCaps {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let names: Vec<String> = Vec::deserialize(deserializer)?;
        let mut caps = Self::NONE;
        for name in &names {
            caps |= match name.as_str() {
                "store" => Self::STORE,
                "developer_role" => Self::DEVELOPER_ROLE,
                "reasoning_effort" => Self::REASONING_EFFORT,
                "usage_in_streaming" => Self::USAGE_IN_STREAMING,
                "tool_result_name" => Self::TOOL_RESULT_NAME,
                "assistant_after_tool_result" => Self::ASSISTANT_AFTER_TOOL_RESULT,
                "reasoning_content_required" => Self::REASONING_CONTENT_REQUIRED,
                "prompt_cache_key" => Self::PROMPT_CACHE_KEY,
                other => {
                    return Err(serde::de::Error::unknown_variant(other, &[
                        "store",
                        "developer_role",
                        "reasoning_effort",
                        "usage_in_streaming",
                        "tool_result_name",
                        "assistant_after_tool_result",
                        "reasoning_content_required",
                        "prompt_cache_key",
                    ]))
                }
            };
        }
        Ok(caps)
    }
}

/// Compatibility flags for OpenAI-compatible providers.
/// Different providers have different quirks even though they share the same base API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiCompat {
    /// Provider capabilities/quirks.
    #[serde(default)]
    pub caps: CompatCaps,
    /// Which field name to use for max tokens.
    pub max_tokens_field: MaxTokensField,
    /// How thinking/reasoning content is formatted in streaming.
    pub thinking_format: ThinkingFormat,
}

impl Default for OpenAiCompat {
    fn default() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING,
            max_tokens_field: MaxTokensField::MaxTokens,
            thinking_format: ThinkingFormat::OpenAi,
        }
    }
}

impl OpenAiCompat {
    pub fn has_cap(&self, cap: CompatCaps) -> bool {
        self.caps.contains(cap)
    }

    /// Compat flags for native OpenAI.
    pub fn openai() -> Self {
        Self {
            caps: CompatCaps::STORE
                | CompatCaps::DEVELOPER_ROLE
                | CompatCaps::REASONING_EFFORT
                | CompatCaps::USAGE_IN_STREAMING
                | CompatCaps::PROMPT_CACHE_KEY,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        }
    }

    /// Compat flags for xAI (Grok).
    pub fn xai() -> Self {
        Self {
            thinking_format: ThinkingFormat::Xai,
            ..Default::default()
        }
    }

    /// Compat flags for the llmproxy Grok CLI Chat Completions adapter.
    ///
    /// This is intentionally separate from direct xAI: api.x.ai's Chat
    /// Completions endpoint rejects `reasoning_effort`, while the Grok CLI
    /// adapter accepts it and translates to the Responses API.
    pub fn grok_cli() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_EFFORT,
            ..Default::default()
        }
    }

    /// Compat flags for Groq.
    pub fn groq() -> Self {
        Self::default()
    }

    /// Compat flags for Cerebras.
    pub fn cerebras() -> Self {
        Self::default()
    }

    /// Compat flags for OpenRouter.
    pub fn openrouter() -> Self {
        Self {
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        }
    }

    /// Compat flags for Mistral.
    pub fn mistral() -> Self {
        Self {
            max_tokens_field: MaxTokensField::MaxTokens,
            ..Default::default()
        }
    }

    /// Compat flags for DeepSeek.
    pub fn deepseek() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_CONTENT_REQUIRED,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        }
    }

    /// Compat flags for Z.ai (Zhipu AI).
    pub fn zai() -> Self {
        Self::default()
    }

    /// Compat flags for MiniMax.
    pub fn minimax() -> Self {
        Self::default()
    }
}

/// Full model configuration. Knows everything needed to make API calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model identifier sent to the API (e.g. "gpt-4o", "claude-sonnet-4-20250514").
    pub id: String,
    /// Human-friendly name.
    pub name: String,
    /// Which API protocol to use.
    pub api: ApiProtocol,
    /// Provider name (e.g. "openai", "anthropic", "xai").
    pub provider: String,
    /// Base URL for API requests (without trailing slash).
    pub base_url: String,
    /// Context window size in tokens.
    pub context_window: u32,
    /// Default max output tokens.
    pub max_tokens: u32,
    /// Whether this concrete model supports reasoning/thinking.
    ///
    /// This is model metadata, separate from [`OpenAiCompat`] which describes
    /// whether a provider endpoint can carry the model's reasoning controls.
    #[serde(default = "default_reasoning_capability")]
    pub reasoning: bool,
    /// Input modalities the model accepts. Defaults to text-only, matching the
    /// conservative assumption for unknown OpenAI-compatible servers. The
    /// request builders consult this before attaching image content, mirroring
    /// pi's `model.input.includes("image")` gate.
    #[serde(default = "default_input_modalities")]
    pub input: Vec<InputModality>,
    /// Cost configuration.
    #[serde(default)]
    pub cost: CostConfig,
    /// Additional headers to send with requests.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// OpenAI-compat quirk flags (only for OpenAiCompletions protocol).
    #[serde(default)]
    pub compat: Option<OpenAiCompat>,
    /// Per-model overrides for the abstract [`ThinkingLevel`] tiers. Keys are
    /// lowercase level names (e.g. `"xhigh"`), and the value encodes three
    /// states (mirroring pi's `thinkingLevelMap`):
    ///
    /// - `Some(effort)` — the level is supported and maps to this exact
    ///   provider effort string (e.g. Opus 4.6 maps `xhigh` to `"max"`).
    /// - `None` — the level is explicitly *unsupported* for this model and is
    ///   omitted from the cycle (e.g. `gpt-5.5-pro` drops `off`/`low`).
    /// - key absent — the level uses the protocol's default behavior.
    #[serde(default)]
    pub thinking_level_map: HashMap<String, Option<String>>,
}

fn default_reasoning_capability() -> bool {
    // Preserve compatibility for externally deserialized model definitions that
    // predate this field. First-party non-reasoning models set it explicitly.
    true
}

fn default_input_modalities() -> Vec<InputModality> {
    vec![InputModality::Text]
}

/// Resolve `(context_window, default_max_output_tokens)` for an Anthropic model.
///
/// Version-gated rather than id-listed so future releases work without edits,
/// mirroring pi's per-model registry values:
/// - Opus 4.6+, Sonnet 4.6+, and the Fable family carry the 1M context window and
///   a 128k output cap.
/// - Other modern Claude 4.x (Sonnet/Haiku/Opus) support a 64k output budget.
/// - Legacy `claude-3-*` and unparseable ids fall back to conservative caps.
///
/// The output cap is an upper bound only — [`StreamConfig::resolved_max_tokens`]
/// clamps it to the remaining context window per request.
fn anthropic_context_window(id: &str) -> (u32, u32) {
    let Some((family, major, minor)) = anthropic_model_version(id) else {
        return (200_000, 8192);
    };
    let million_ctx = family == "fable"
        || (family == "opus" && (major, minor) >= (4, 6))
        || (family == "sonnet" && ((major, minor) >= (4, 6) || major >= 5));
    if million_ctx {
        return (1_000_000, 128_000);
    }
    if major >= 4 {
        return (200_000, 64_000);
    }
    (200_000, 8192)
}

/// Default `thinking_level_map` for an Anthropic model id.
///
/// Only model-specific selectable tiers are recorded. Current adaptive-thinking
/// models opt into `max`; Opus 4.7+, Sonnet 5+, and Fable also opt into
/// `xhigh`. Opus 4.6 exposes `max` but not `xhigh`, while Fable cannot disable
/// thinking. Mirrors pi's per-model `thinkingLevelMap`.
fn anthropic_thinking_level_map(id: &str) -> HashMap<String, Option<String>> {
    let mut map = HashMap::new();
    if let Some((family, major, minor)) = anthropic_model_version(id) {
        let adaptive = family == "fable"
            || (family == "opus" && (major, minor) >= (4, 6))
            || (family == "sonnet" && ((major, minor) >= (4, 6) || major >= 5));
        if adaptive {
            map.insert("max".into(), Some("max".into()));
        }
        let supports_xhigh = family == "fable"
            || (family == "opus" && (major, minor) >= (4, 7))
            || (family == "sonnet" && major >= 5);
        if supports_xhigh {
            map.insert("xhigh".into(), Some("xhigh".into()));
        } else if adaptive {
            // Backward compatibility for persisted/configured `xhigh`: max-only
            // models should receive a valid effort even though xhigh is no
            // longer offered as a separate selectable tier.
            map.insert("xhigh".into(), Some("max".into()));
        }
        if family == "fable" {
            map.insert("off".into(), None);
        }
    }
    map
}

/// Parse an Anthropic model's family and `(major, minor)` version from ids using
/// the modern `claude-<family>-<major>-<minor>` / `claude-<family>-<major>.<minor>`
/// scheme (e.g. `claude-opus-4-6`, `claude-opus-4.6-20251101`).
///
/// A trailing date (8-digit run) is ignored, and a missing minor defaults to 0
/// (e.g. `claude-opus-4` -> `(opus, 4, 0)`). A numeric component longer than two
/// digits is treated as a date, not a version, so `claude-opus-4-20250514` parses
/// as `(opus, 4, 0)`. Legacy ids that place the version before the family (e.g.
/// `claude-3-opus`) yield no version and fall back to defaults — they predate the
/// features gated here.
fn anthropic_model_version(id: &str) -> Option<(&'static str, u32, u32)> {
    let normalized = id.trim().to_ascii_lowercase();
    let family = ["opus", "sonnet", "haiku", "fable"]
        .into_iter()
        .find(|f| normalized.contains(*f))?;
    let after = normalized.split(family).nth(1)?;
    // Short numeric component = version part; a longer run is a date suffix.
    let mut parts = after
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| (1..=2).contains(&s.len()));
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some((family, major, minor))
}

struct GrokModelMetadata {
    context_window: u32,
    max_tokens: u32,
    reasoning: bool,
    thinking_level_map: HashMap<String, Option<String>>,
}

fn grok_model_metadata(id: &str) -> Option<GrokModelMetadata> {
    let normalized = id.trim().to_ascii_lowercase();
    let model_id = normalized
        .strip_prefix("xai/")
        .or_else(|| normalized.strip_prefix("x-ai/"))
        .unwrap_or(&normalized);
    let mut levels = HashMap::new();
    match model_id {
        // Grok CLI advertises a 500k context window and low/medium/high
        // reasoning efforts. It does not publish a separate output cap, so the
        // context clamp is the authoritative output limit.
        "grok-4.5" => {
            levels.insert("off".into(), None);
            levels.insert("minimal".into(), None);
            levels.insert("low".into(), Some("low".into()));
            levels.insert("medium".into(), Some("medium".into()));
            levels.insert("high".into(), Some("high".into()));
            levels.insert("adaptive".into(), Some("high".into()));
            levels.insert("xhigh".into(), None);
            levels.insert("max".into(), None);
            Some(GrokModelMetadata {
                context_window: 500_000,
                max_tokens: 500_000,
                reasoning: true,
                thinking_level_map: levels,
            })
        }
        "grok-composer-2.5-fast" => Some(GrokModelMetadata {
            context_window: 200_000,
            max_tokens: 200_000,
            reasoning: false,
            thinking_level_map: levels,
        }),
        _ => None,
    }
}

fn openai_context_window(id: &str) -> u32 {
    match native_openai_model_id(id).as_str() {
        // Direct OpenAI Responses metadata currently reports this 272k catalog
        // window. Evot uses the Chat Completions transport, so treat the value
        // as the configured budgeting limit; Codex separately reports 372k.
        "gpt-5.4" | "gpt-5.5" | "gpt-5.6-luna" | "gpt-5.6-sol" | "gpt-5.6-terra" => 272_000,
        "gpt-5.4-pro" | "gpt-5.5-pro" => 1_050_000,
        #[cfg(test)]
        "tiny-context" => 128,
        _ => 128_000,
    }
}

fn openai_max_tokens(id: &str) -> u32 {
    match native_openai_model_id(id).as_str() {
        "gpt-5.4" | "gpt-5.4-pro" | "gpt-5.5" | "gpt-5.5-pro" | "gpt-5.6-luna" | "gpt-5.6-sol"
        | "gpt-5.6-terra" => 128_000,
        _ => 32_768,
    }
}

/// Strip an OpenRouter-style `openai/` prefix before applying model-family
/// reasoning metadata and user-configured generic-provider catalog limits. The
/// provider still receives the original id unchanged.
fn native_openai_model_id(id: &str) -> String {
    let normalized = id.trim().to_ascii_lowercase();
    normalized
        .strip_prefix("openai/")
        .unwrap_or(&normalized)
        .to_string()
}

/// Default `thinking_level_map` for native OpenAI/Codex-backed GPT models.
///
/// Codex model metadata has no `adaptive` effort. It resolves an unspecified
/// effort to the model's default. Mirror that by mapping Evot's `adaptive` to
/// the known default for GPT/Codex models, while allowing `xhigh` to pass
/// through for models that advertise it. Per-model tiers that the API rejects
/// are mapped to `None` so they drop out of the selectable cycle.
fn openai_thinking_level_map(id: &str) -> HashMap<String, Option<String>> {
    let mut map = HashMap::new();
    let normalized = native_openai_model_id(id);
    if normalized.starts_with("gpt-5") || normalized.starts_with("codex-") {
        let default = if normalized == "gpt-5.4" {
            "xhigh"
        } else {
            "medium"
        };
        map.insert("adaptive".into(), Some(default.into()));
        if normalized.starts_with("gpt-5.6-") {
            map.insert("off".into(), Some("none".into()));
            map.insert("xhigh".into(), Some("xhigh".into()));
            map.insert("max".into(), Some("max".into()));
        } else {
            map.insert("xhigh".into(), Some("xhigh".into()));
        }
        // gpt-5.5-pro rejects the lowest tiers; medium is its floor. gpt-5.5
        // (non-pro) drops only `minimal`. Values mirror pi's per-model
        // `thinkingLevelMap` so the unsupported tiers match upstream exactly.
        if normalized.ends_with("gpt-5.5-pro") {
            map.insert("off".into(), None);
            map.insert("minimal".into(), None);
            map.insert("low".into(), None);
        } else if normalized == "gpt-5.5" {
            map.insert("minimal".into(), None);
        }
    }
    map
}

impl ModelConfig {
    /// Thinking levels a user can cycle through for this model, in ascending
    /// order of effort.
    ///
    /// The ramp is `off → low → medium → high → xhigh → max`, filtered by
    /// [`Self::level_selectable`] so that per-model overrides in
    /// [`Self::thinking_level_map`] (a `None` value) remove unsupported tiers.
    ///
    /// OpenAI-compatible models only honor a reasoning effort when the provider
    /// advertises [`CompatCaps::REASONING_EFFORT`]; without it the effort is
    /// inert and the list is empty. `Minimal` and `Adaptive` are never offered:
    /// they alias other tiers and would be redundant stops in the cycle.
    pub fn supported_thinking_levels(&self) -> Vec<ThinkingLevel> {
        use crate::ThinkingLevel::*;
        if !self.reasoning {
            return Vec::new();
        }
        if self.api == ApiProtocol::OpenAiCompletions && !self.honors_reasoning_effort() {
            return Vec::new();
        }
        [Off, Low, Medium, High, Xhigh, Max]
            .into_iter()
            .filter(|level| self.level_selectable(*level))
            .collect()
    }

    /// Whether the OpenAI-compatible provider sends a `reasoning_effort` field
    /// at all. Always true for the Anthropic/Bedrock protocols.
    fn honors_reasoning_effort(&self) -> bool {
        match self.api {
            ApiProtocol::AnthropicMessages | ApiProtocol::BedrockConverseStream => true,
            ApiProtocol::OpenAiCompletions => self
                .compat
                .as_ref()
                .map(|c| c.caps.contains(CompatCaps::REASONING_EFFORT))
                .unwrap_or(false),
        }
    }

    /// Whether `level` is offered in the cycle for this model, per
    /// [`Self::thinking_level_map`]:
    ///
    /// - value `Some(effort)` — supported (explicit effort mapping).
    /// - value `None` — explicitly unsupported.
    /// - key absent — protocol default: every ordinary tier is selectable;
    ///   `xhigh` and `max` require an explicit per-model mapping.
    fn level_selectable(&self, level: ThinkingLevel) -> bool {
        match self.thinking_level_map.get(level.as_str()) {
            Some(Some(effort)) => {
                // A legacy xhigh→max alias keeps old persisted settings valid,
                // but max is the only selectable tier when both keys map there.
                !(level == ThinkingLevel::Xhigh
                    && effort == "max"
                    && self.thinking_level_map.contains_key("max"))
            }
            Some(None) => false,
            None => !matches!(level, ThinkingLevel::Xhigh | ThinkingLevel::Max),
        }
    }

    /// The explicit per-model effort string for `level`, if the model maps one
    /// (e.g. `max` → `"max"` on GPT-5.6). `None` means "no override" — the
    /// caller applies its protocol default. Shared by the request builders so
    /// the mapping lives in exactly one place.
    pub fn thinking_effort_override(&self, level: ThinkingLevel) -> Option<&str> {
        self.thinking_level_map.get(level.as_str())?.as_deref()
    }

    /// Whether the `off` level can be expressed to the provider. `false` only
    /// when the model maps `off` to `None` in its [`Self::thinking_level_map`],
    /// meaning reasoning cannot be turned off for this model (mirrors pi's
    /// `thinkingLevelMap?.off !== null` gate). Callers that would otherwise emit
    /// a "disable thinking" request should omit the field instead.
    pub fn can_disable_thinking(&self) -> bool {
        !matches!(self.thinking_level_map.get("off"), Some(None))
    }

    /// Whether this model accepts image input. Mirrors pi's
    /// `model.input.includes("image")` gate used by the request builders and
    /// the read tool to decide whether to attach or drop image content.
    pub fn supports_image(&self) -> bool {
        self.input.contains(&InputModality::Image)
    }

    /// Create a new Anthropic model config.
    pub fn anthropic(id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let (context_window, max_tokens) = anthropic_context_window(&id);
        let thinking_level_map = anthropic_thinking_level_map(&id);
        Self {
            id,
            name: name.into(),
            api: ApiProtocol::AnthropicMessages,
            provider: "anthropic".into(),
            base_url: "https://api.anthropic.com".into(),
            context_window,
            max_tokens,
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: None,
            thinking_level_map,
        }
    }

    /// Create a new OpenAI model config.
    pub fn openai(id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let context_window = openai_context_window(&id);
        let max_tokens = openai_max_tokens(&id);
        let thinking_level_map = openai_thinking_level_map(&id);
        Self {
            id,
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            context_window,
            max_tokens,
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::openai()),
            thinking_level_map,
        }
    }

    /// Create a config for an OpenAI-compatible server (the runtime path for
    /// every OpenAI-protocol provider — DeepSeek, xAI, Groq, Z.ai, MiniMax,
    /// local servers, etc.). Provider-specific base URL, compat flags and
    /// context window are layered on by the app's `build_model_config`.
    ///
    /// `max_tokens` is a generous default output cap; the request builder
    /// clamps it to the remaining context window per call, so it never
    /// overflows the window. Mirrors pi's generous per-model caps + clamp.
    pub fn local(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        let id = model_id.into();
        let metadata = grok_model_metadata(&id).unwrap_or_else(|| GrokModelMetadata {
            context_window: openai_context_window(&id),
            max_tokens: openai_max_tokens(&id),
            reasoning: true,
            thinking_level_map: openai_thinking_level_map(&id),
        });
        Self {
            id,
            name: "Local Model".into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "local".into(),
            base_url: base_url.into(),
            context_window: metadata.context_window,
            max_tokens: metadata.max_tokens,
            reasoning: metadata.reasoning,
            input: default_input_modalities(),
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::default()),
            thinking_level_map: metadata.thinking_level_map,
        }
    }
}
