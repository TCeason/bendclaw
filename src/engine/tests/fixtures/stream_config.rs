//! Builder for `StreamConfig` and stream event collection utilities.

use evotengine::provider::model::ModelConfig;
use evotengine::provider::traits::*;
use evotengine::types::*;

/// Builder for `StreamConfig` with sensible defaults.
pub struct StreamConfigBuilder {
    model: String,
    system_prompt: String,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
    thinking_level: ThinkingLevel,
    api_key: String,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    model_config: Option<ModelConfig>,
    cache_config: CacheConfig,
}

impl StreamConfigBuilder {
    /// Create a builder with minimal defaults.
    pub fn new() -> Self {
        Self {
            model: "test-model".into(),
            system_prompt: String::new(),
            messages: vec![Message::user("Hello")],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig::default(),
        }
    }

    /// Anthropic-flavored defaults.
    pub fn anthropic() -> Self {
        Self::new()
            .model("claude-sonnet-4-20250514")
            .api_key("test-key")
    }

    /// OpenAI-flavored defaults.
    pub fn openai() -> Self {
        Self::new()
            .model("gpt-4o")
            .model_config(ModelConfig::openai("gpt-4o", "GPT-4o"))
    }

    pub fn model(mut self, model: &str) -> Self {
        self.model = model.into();
        self
    }

    pub fn system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn thinking(mut self, level: ThinkingLevel) -> Self {
        self.thinking_level = level;
        self
    }

    pub fn api_key(mut self, key: &str) -> Self {
        self.api_key = key.into();
        self
    }

    pub fn max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    pub fn no_max_tokens(mut self) -> Self {
        self.max_tokens = None;
        self
    }

    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn model_config(mut self, config: ModelConfig) -> Self {
        self.model_config = Some(config);
        self
    }

    pub fn cache_config(mut self, config: CacheConfig) -> Self {
        self.cache_config = config;
        self
    }

    pub fn cache_disabled(mut self) -> Self {
        self.cache_config = CacheConfig {
            enabled: false,
            strategy: CacheStrategy::Disabled,
        };
        self
    }

    pub fn build(self) -> StreamConfig {
        StreamConfig {
            model: self.model,
            system_prompt: self.system_prompt,
            messages: self.messages,
            tools: self.tools,
            thinking_level: self.thinking_level,
            api_key: self.api_key,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            model_config: self.model_config,
            cache_config: self.cache_config,
        }
    }
}

/// Collect all `StreamEvent`s from an unbounded receiver.
pub fn collect_stream_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<StreamEvent>,
) -> Vec<StreamEvent> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// Shorthand: create a tool definition.
pub fn tool_def(name: &str, desc: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: serde_json::json!({"type": "object"}),
    }
}
