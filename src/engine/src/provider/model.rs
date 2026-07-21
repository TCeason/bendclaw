//! Model configuration and provider compatibility flags.
//!
//! Three independent layers compose a runtime config:
//! 1. Model catalog ([`super::catalog`]) — intrinsic per-model capabilities
//! 2. Provider transport ([`OpenAiCompat`]) — endpoint protocol quirks
//! 3. Explicit overrides — env/user authority over either layer

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use super::catalog::ModelMetadata;
use super::catalog::{self};

/// Which API protocol a model uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProtocol {
    AnthropicMessages,
    OpenAiResponses,
    OpenAiCompletions,
    BedrockConverseStream,
}

impl std::fmt::Display for ApiProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnthropicMessages => write!(f, "anthropic_messages"),
            Self::OpenAiResponses => write!(f, "openai_responses"),
            Self::OpenAiCompletions => write!(f, "openai_completions"),
            Self::BedrockConverseStream => write!(f, "bedrock_converse_stream"),
        }
    }
}

/// A modality a model accepts as input.
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
    OpenRouter,
    DeepSeek,
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

/// Compatibility flags for OpenAI-compatible endpoints.
///
/// This is transport metadata: what a given channel can carry on the wire.
/// It is independent of whether a concrete model supports reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiCompat {
    #[serde(default)]
    pub caps: CompatCaps,
    pub max_tokens_field: MaxTokensField,
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

    pub fn xai() -> Self {
        Self {
            thinking_format: ThinkingFormat::Xai,
            ..Default::default()
        }
    }

    /// llmproxy Grok CLI Chat Completions adapter.
    ///
    /// Distinct from direct xAI: the CLI adapter accepts `reasoning_effort` and
    /// translates to the Responses API, while api.x.ai Chat Completions rejects it.
    pub fn grok_cli() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_EFFORT,
            ..Default::default()
        }
    }

    pub fn groq() -> Self {
        Self::default()
    }

    pub fn cerebras() -> Self {
        Self::default()
    }

    pub fn openrouter() -> Self {
        Self {
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            thinking_format: ThinkingFormat::OpenRouter,
            ..Default::default()
        }
    }

    pub fn mistral() -> Self {
        Self {
            max_tokens_field: MaxTokensField::MaxTokens,
            ..Default::default()
        }
    }

    pub fn deepseek() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_CONTENT_REQUIRED,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            thinking_format: ThinkingFormat::DeepSeek,
        }
    }

    pub fn moonshot() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_CONTENT_REQUIRED,
            max_tokens_field: MaxTokensField::MaxTokens,
            thinking_format: ThinkingFormat::DeepSeek,
        }
    }

    pub fn zai() -> Self {
        Self::default()
    }

    pub fn minimax() -> Self {
        Self::default()
    }

    /// Transport profile for a named OpenAI-compatible channel.
    pub fn for_provider(provider: &str) -> Self {
        match provider {
            "openai" => Self::openai(),
            "deepseek" => Self::deepseek(),
            "xai" => Self::xai(),
            "grok" => Self::grok_cli(),
            "groq" => Self::groq(),
            "cerebras" => Self::cerebras(),
            "openrouter" => Self::openrouter(),
            "moonshotai" | "moonshotai-cn" => Self::moonshot(),
            "mistral" => Self::mistral(),
            "zai" => Self::zai(),
            "minimax" => Self::minimax(),
            _ => Self::default(),
        }
    }
}

/// Full model configuration used at request time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub api: ApiProtocol,
    pub provider: String,
    pub base_url: String,
    pub context_window: u32,
    pub max_tokens: u32,
    #[serde(default = "default_reasoning_capability")]
    pub reasoning: bool,
    #[serde(default = "default_input_modalities")]
    pub input: Vec<InputModality>,
    #[serde(default)]
    pub cost: CostConfig,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub compat: Option<OpenAiCompat>,
    /// Per-model thinking tier map. Values:
    /// - `Some(effort)` — supported, maps to this wire string
    /// - `None` — explicitly unsupported
    /// - key absent — protocol default
    #[serde(default)]
    pub thinking_level_map: HashMap<String, Option<String>>,
    /// Anthropic adaptive thinking (`thinking.type: adaptive`).
    #[serde(default)]
    pub force_adaptive_thinking: bool,
    /// Whether this model accepts a temperature parameter.
    #[serde(default = "default_supports_temperature")]
    pub supports_temperature: bool,
}

fn default_reasoning_capability() -> bool {
    true
}

fn default_input_modalities() -> Vec<InputModality> {
    vec![InputModality::Text]
}

fn default_supports_temperature() -> bool {
    true
}

fn protocol_defaults(api: ApiProtocol) -> ModelMetadata {
    match api {
        ApiProtocol::AnthropicMessages | ApiProtocol::BedrockConverseStream => {
            ModelMetadata::vision(200_000, 8192)
        }
        ApiProtocol::OpenAiResponses | ApiProtocol::OpenAiCompletions => {
            ModelMetadata::text_only(128_000, 32_768)
        }
    }
}

fn supports_openai_none_reasoning(id: &str) -> bool {
    matches!(
        id,
        "gpt-5.1"
            | "gpt-5.2"
            | "gpt-5.3-codex"
            | "gpt-5.4"
            | "gpt-5.4-mini"
            | "gpt-5.4-nano"
            | "gpt-5.5"
            | "gpt-5.6-sol"
            | "gpt-5.6-terra"
            | "gpt-5.6-luna"
    )
}

impl ModelConfig {
    /// Compose a runtime config from protocol defaults + model catalog + transport.
    pub fn resolve(
        api: ApiProtocol,
        provider: impl Into<String>,
        model_id: impl Into<String>,
        name: impl Into<String>,
        base_url: impl Into<String>,
        compat: Option<OpenAiCompat>,
    ) -> Self {
        let id = model_id.into();
        let provider = provider.into();
        let normalized_id = catalog::normalize_model_id(&id);
        let family_id = normalized_id
            .strip_prefix("moonshotai/")
            .unwrap_or(&normalized_id);
        let metadata = catalog::resolve(&id).unwrap_or_else(|| protocol_defaults(api));
        let mut thinking_level_map = metadata.thinking_level_map;

        // Kimi K3 uses the same sparse reasoning ramp through Kimi Coding,
        // Moonshot, and OpenRouter. Context/output limits remain independently
        // sourced because transport catalogs can advertise different limits.
        if matches!(family_id, "k3" | "kimi-k3") {
            thinking_level_map = catalog::kimi_k3_thinking_level_map();
        }

        // pi applies these constraints after composing model + provider + API.
        if provider == "openai" && normalized_id == "gpt-5.5" {
            thinking_level_map.insert("minimal".into(), None);
        }
        if api == ApiProtocol::OpenAiResponses
            && provider == "openai"
            && supports_openai_none_reasoning(&normalized_id)
        {
            thinking_level_map.insert("off".into(), Some("none".into()));
        }

        Self {
            id,
            name: name.into(),
            api,
            provider,
            base_url: base_url.into(),
            context_window: metadata.context_window,
            max_tokens: metadata.max_tokens,
            reasoning: metadata.reasoning,
            input: metadata.input,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat,
            thinking_level_map,
            force_adaptive_thinking: metadata.force_adaptive_thinking,
            supports_temperature: metadata.supports_temperature,
        }
    }

    pub fn anthropic(id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let name = name.into();
        Self::resolve(
            ApiProtocol::AnthropicMessages,
            "anthropic",
            id.clone(),
            name,
            "https://api.anthropic.com",
            None,
        )
    }

    /// OpenAI-compatible Chat Completions configuration.
    pub fn openai(id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let name = name.into();
        Self::resolve(
            ApiProtocol::OpenAiCompletions,
            "openai",
            id.clone(),
            name,
            "https://api.openai.com/v1",
            Some(OpenAiCompat::openai()),
        )
    }

    /// Native OpenAI Responses configuration.
    pub fn openai_responses(id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let name = name.into();
        Self::resolve(
            ApiProtocol::OpenAiResponses,
            "openai",
            id.clone(),
            name,
            "https://api.openai.com/v1",
            Some(OpenAiCompat::openai()),
        )
    }

    /// OpenAI-compatible channel with default transport profile.
    ///
    /// Model capabilities still come from the catalog; this only sets the
    /// protocol default and a conservative text-only fallback for unknown ids.
    pub fn local(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        let id = model_id.into();
        Self::resolve(
            ApiProtocol::OpenAiCompletions,
            "local",
            id.clone(),
            "Local Model",
            base_url,
            Some(OpenAiCompat::default()),
        )
    }

    pub fn supports_image(&self) -> bool {
        self.input.contains(&InputModality::Image)
    }
}
