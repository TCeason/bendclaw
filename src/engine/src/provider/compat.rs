use serde::Deserialize;
use serde::Serialize;

/// How a provider handles the output-token field.
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

/// Capabilities of an OpenAI-compatible transport.
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
    /// Route accepts the OpenAI verbosity extension.
    pub const VERBOSITY: Self = Self(1 << 8);
    /// Responses route accepts provider-native remote compaction.
    pub const REMOTE_COMPACTION: Self = Self(1 << 9);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Resolve one serialized/configured capability name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "store" => Some(Self::STORE),
            "developer_role" => Some(Self::DEVELOPER_ROLE),
            "reasoning_effort" => Some(Self::REASONING_EFFORT),
            "usage_in_streaming" => Some(Self::USAGE_IN_STREAMING),
            "tool_result_name" => Some(Self::TOOL_RESULT_NAME),
            "assistant_after_tool_result" => Some(Self::ASSISTANT_AFTER_TOOL_RESULT),
            "reasoning_content_required" => Some(Self::REASONING_CONTENT_REQUIRED),
            "prompt_cache_key" => Some(Self::PROMPT_CACHE_KEY),
            "verbosity" => Some(Self::VERBOSITY),
            "remote_compaction" => Some(Self::REMOTE_COMPACTION),
            _ => None,
        }
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
            (Self::VERBOSITY, "verbosity"),
            (Self::REMOTE_COMPACTION, "remote_compaction"),
        ];
        let count = all.iter().filter(|(flag, _)| self.contains(*flag)).count();
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
            let Some(cap) = Self::from_name(name) else {
                return Err(serde::de::Error::unknown_variant(name, &[
                    "store",
                    "developer_role",
                    "reasoning_effort",
                    "usage_in_streaming",
                    "tool_result_name",
                    "assistant_after_tool_result",
                    "reasoning_content_required",
                    "prompt_cache_key",
                    "verbosity",
                    "remote_compaction",
                ]));
            };
            caps |= cap;
        }
        Ok(caps)
    }
}

/// Whether a route targets OpenAI's canonical first-party API.
pub fn is_official_openai_route(provider: &str, base_url: &str) -> bool {
    provider == "openai" && base_url.trim_end_matches('/') == "https://api.openai.com/v1"
}

/// Routes known to expose OpenAI Responses-native extensions. Keep this
/// allowlist narrow: unknown same-named proxies remain conservative.
pub fn is_native_openai_responses_route(provider: &str, base_url: &str) -> bool {
    if provider != "openai" {
        return false;
    }
    matches!(
        base_url.trim_end_matches('/'),
        "https://api.openai.com/v1" | "https://openrouter.databend.cloud/openai/v1"
    )
}

/// Compatibility metadata for an OpenAI-compatible endpoint.
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

    /// Resolve transport behavior from provider identity. Endpoint-native
    /// features such as verbosity and remote compaction are resolved
    /// separately by `RouteCapabilities`.
    pub fn for_route(provider: &str, _base_url: &str) -> Self {
        Self::for_provider(provider)
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

    fn for_provider(provider: &str) -> Self {
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
