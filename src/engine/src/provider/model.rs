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

/// Provider/model-level thinking passback policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingPassbackPolicy {
    /// Never send prior thinking blocks back to the provider.
    #[default]
    Disabled,
    /// Preserve thinking on assistant tool-use messages retained in history.
    ToolUseMessages,
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
    /// Cost configuration.
    #[serde(default)]
    pub cost: CostConfig,
    /// Additional headers to send with requests.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// OpenAI-compat quirk flags (only for OpenAiCompletions protocol).
    #[serde(default)]
    pub compat: Option<OpenAiCompat>,
    /// Whether prior thinking blocks must be passed back to the provider.
    #[serde(default)]
    pub thinking_passback: ThinkingPassbackPolicy,
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

fn anthropic_context_window(id: &str) -> (u32, u32) {
    // Opus 4.6 introduced the 1M-token context window; every newer Opus keeps
    // it. Version-gating (rather than listing ids) means future Opus releases
    // work without edits.
    if let Some((family, major, minor)) = anthropic_model_version(id) {
        if family == "opus" && (major, minor) >= (4, 6) {
            return (1_000_000, 128_000);
        }
    }
    (200_000, 8192)
}

/// Default `thinking_level_map` for an Anthropic model id.
///
/// The map records only *exceptions* to the request builder's default, which
/// already maps the `xhigh` level to `"xhigh"` effort. The one known exception
/// is Opus 4.6, whose strongest tier is `"max"` rather than `"xhigh"`. Opus 4.7+
/// and all other models use the default, so they need no entry — new models keep
/// working without touching this table. Mirrors pi's per-model `thinkingLevelMap`.
fn anthropic_thinking_level_map(id: &str) -> HashMap<String, Option<String>> {
    let mut map = HashMap::new();
    if let Some((family, major, minor)) = anthropic_model_version(id) {
        if family == "opus" && (major, minor) == (4, 6) {
            map.insert("xhigh".into(), Some("max".into()));
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
    let family = ["opus", "sonnet", "haiku"]
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

fn openai_context_window(id: &str) -> u32 {
    match id.trim().to_ascii_lowercase().as_str() {
        // gpt-5.5's Codex backend only serves ~272k usable input (matches pi-mono),
        // not the advertised 400k.
        "gpt-5.5" => 272_000,
        #[cfg(test)]
        "tiny-context" => 128,
        _ => 128_000,
    }
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
    let normalized = id.trim().to_ascii_lowercase();
    if normalized.starts_with("gpt-5") || normalized.starts_with("codex-") {
        let default = if normalized == "gpt-5.4" {
            "xhigh"
        } else {
            "medium"
        };
        map.insert("adaptive".into(), Some(default.into()));
        map.insert("xhigh".into(), Some("xhigh".into()));
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
    /// The ramp is `off → low → medium → high → xhigh`, filtered by
    /// [`Self::level_selectable`] so that per-model overrides in
    /// [`Self::thinking_level_map`] (a `None` value) remove unsupported tiers.
    ///
    /// OpenAI-compatible models only honor a reasoning effort when the provider
    /// advertises [`CompatCaps::REASONING_EFFORT`]; without it the effort is
    /// inert and the list is empty. `Minimal` and `Adaptive` are never offered:
    /// they alias other tiers and would be redundant stops in the cycle.
    pub fn supported_thinking_levels(&self) -> Vec<ThinkingLevel> {
        use crate::ThinkingLevel::*;
        if self.api == ApiProtocol::OpenAiCompletions && !self.honors_reasoning_effort() {
            return Vec::new();
        }
        [Off, Low, Medium, High, Xhigh]
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
    /// - key absent — protocol default: every tier is selectable except
    ///   `xhigh` on OpenAI, which collapses onto `high` unless mapped.
    fn level_selectable(&self, level: ThinkingLevel) -> bool {
        match self.thinking_level_map.get(level.as_str()) {
            Some(Some(_)) => true,
            Some(None) => false,
            None => level != ThinkingLevel::Xhigh || self.api != ApiProtocol::OpenAiCompletions,
        }
    }

    /// The explicit per-model effort string for `level`, if the model maps one
    /// (e.g. `xhigh` → `"max"` on Opus 4.6). `None` means "no override" — the
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

    pub fn apply_inferred_capabilities(&mut self) {
        if self.api == ApiProtocol::AnthropicMessages && self.requires_tool_use_thinking_passback()
        {
            self.thinking_passback = ThinkingPassbackPolicy::ToolUseMessages;
        }
    }

    fn requires_tool_use_thinking_passback(&self) -> bool {
        let id = self.id.trim_start().to_ascii_lowercase();
        id.starts_with("deepseek") || id.starts_with("kimi")
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
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: None,
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map,
        }
    }

    /// Create a new OpenAI model config.
    pub fn openai(id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let context_window = openai_context_window(&id);
        let thinking_level_map = openai_thinking_level_map(&id);
        Self {
            id,
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            context_window,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::openai()),
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map,
        }
    }

    /// Create a config for a local OpenAI-compatible server (LM Studio, Ollama, etc.).
    /// No API key required — sends an empty Bearer token.
    pub fn local(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        let id = model_id.into();
        let context_window = openai_context_window(&id);
        let thinking_level_map = openai_thinking_level_map(&id);
        Self {
            id,
            name: "Local Model".into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "local".into(),
            base_url: base_url.into(),
            context_window,
            max_tokens: 16384,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::default()),
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map,
        }
    }

    /// Create a new Z.ai (Zhipu AI) model config.
    ///
    /// Models: `glm-4.7`, `glm-4.5-air`, `glm-5`, etc.
    pub fn zai(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "zai".into(),
            base_url: "https://api.z.ai/api/paas/v4".into(),
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::zai()),
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map: HashMap::new(),
        }
    }

    /// Create a new MiniMax model config.
    ///
    /// Models: `MiniMax-Text-01`, `MiniMax-M1`, etc.
    pub fn minimax(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "minimax".into(),
            base_url: "https://api.minimaxi.chat/v1".into(),
            context_window: 1_000_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::minimax()),
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map: HashMap::new(),
        }
    }

    /// Create a new xAI (Grok) model config.
    ///
    /// Models: `grok-3-mini`, `grok-3`, etc.
    pub fn xai(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "xai".into(),
            base_url: "https://api.x.ai/v1".into(),
            context_window: 131_072,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::xai()),
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map: HashMap::new(),
        }
    }

    /// Create a new Groq model config.
    ///
    /// Models: `llama-3.3-70b-versatile`, `mixtral-8x7b-32768`, etc.
    pub fn groq(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::groq()),
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map: HashMap::new(),
        }
    }

    /// Create a new DeepSeek model config.
    ///
    /// Models: `deepseek-chat`, `deepseek-reasoner`, etc.
    pub fn deepseek(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::deepseek()),
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map: HashMap::new(),
        }
    }

    /// Create a new Mistral model config.
    ///
    /// Models: `mistral-large-latest`, `mistral-small-latest`, etc.
    pub fn mistral(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "mistral".into(),
            base_url: "https://api.mistral.ai/v1".into(),
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::mistral()),
            thinking_passback: ThinkingPassbackPolicy::default(),
            thinking_level_map: HashMap::new(),
        }
    }
}
