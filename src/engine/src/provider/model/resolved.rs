use std::collections::HashMap;

use super::capabilities::InputModality;
use super::capabilities::ModelCapabilities;
use super::capabilities::ThinkingLevelPolicy;
use super::capabilities::Verbosity;
use super::overrides::ModelOverrides;
use crate::provider::route::default_base_url;
use crate::provider::route::ApiProtocol;
use crate::provider::route::OpenAiCompat;
use crate::provider::route::RouteCapabilities;
use crate::provider::route::RouteCapabilityOverrides;
use crate::ThinkingLevel;

#[derive(Debug, Clone)]
pub(super) struct ModelRoute {
    pub(super) protocol: ApiProtocol,
    pub(super) provider: String,
    pub(super) base_url: String,
    pub(super) headers: HashMap<String, String>,
    pub(super) compat: Option<OpenAiCompat>,
    pub(super) capabilities: RouteCapabilities,
}

/// Runtime model produced by catalog resolution + route composition + overrides.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub(super) id: String,
    pub(super) route: ModelRoute,
    pub(super) capabilities: ModelCapabilities,
}

/// Inputs needed to produce a runtime model.
#[derive(Debug, Clone)]
pub struct ResolveModelRequest {
    pub protocol: ApiProtocol,
    pub provider: String,
    pub model_id: String,
    pub base_url: String,
    pub headers: HashMap<String, String>,
    pub compat: Option<OpenAiCompat>,
    pub route_capabilities: RouteCapabilities,
    pub overrides: ModelOverrides,
}

impl ModelConfig {
    pub fn resolve(request: ResolveModelRequest) -> Self {
        super::resolver::resolve(request)
    }

    pub fn anthropic(id: impl Into<String>, _display_name: impl Into<String>) -> Self {
        Self::preset(ApiProtocol::AnthropicMessages, "anthropic", id.into(), None)
    }

    pub fn openai(id: impl Into<String>, _display_name: impl Into<String>) -> Self {
        Self::preset(
            ApiProtocol::OpenAiCompletions,
            "openai",
            id.into(),
            Some(OpenAiCompat::openai()),
        )
    }

    pub fn openai_responses(id: impl Into<String>, _display_name: impl Into<String>) -> Self {
        Self::preset(
            ApiProtocol::OpenAiResponses,
            "openai",
            id.into(),
            Some(OpenAiCompat::openai()),
        )
    }

    fn preset(
        protocol: ApiProtocol,
        provider: &str,
        model_id: String,
        compat: Option<OpenAiCompat>,
    ) -> Self {
        let base_url = default_base_url(protocol, provider).to_string();
        let route_capabilities = RouteCapabilities::for_route(
            protocol,
            provider,
            &base_url,
            RouteCapabilityOverrides::default(),
        );
        Self::resolve(ResolveModelRequest {
            protocol,
            provider: provider.into(),
            model_id,
            base_url,
            headers: HashMap::new(),
            compat,
            route_capabilities,
            overrides: ModelOverrides::default(),
        })
    }

    pub fn local(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self::resolve(ResolveModelRequest {
            protocol: ApiProtocol::OpenAiCompletions,
            provider: "local".into(),
            model_id: model_id.into(),
            base_url: base_url.into(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::default()),
            route_capabilities: RouteCapabilities::default(),
            overrides: ModelOverrides::default(),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn protocol(&self) -> ApiProtocol {
        self.route.protocol
    }

    pub fn provider(&self) -> &str {
        &self.route.provider
    }

    pub fn base_url(&self) -> &str {
        &self.route.base_url
    }

    pub fn headers(&self) -> &HashMap<String, String> {
        &self.route.headers
    }

    pub fn compat(&self) -> Option<&OpenAiCompat> {
        self.route.compat.as_ref()
    }

    pub fn route_capabilities(&self) -> RouteCapabilities {
        self.route.capabilities
    }

    pub fn context_window(&self) -> u32 {
        self.capabilities.context_window
    }

    pub fn max_tokens(&self) -> u32 {
        self.capabilities.max_output_tokens
    }

    pub fn reasoning(&self) -> bool {
        self.capabilities.reasoning.supported()
    }

    pub fn input(&self) -> &[InputModality] {
        &self.capabilities.input
    }

    pub fn supports_image(&self) -> bool {
        self.capabilities.supports_image()
    }

    pub fn effective_verbosity(&self) -> Option<Verbosity> {
        self.route
            .capabilities
            .verbosity
            .then_some(self.capabilities.default_verbosity)
            .flatten()
    }

    pub fn can_remote_compact(&self) -> bool {
        self.remote_compaction_unavailable_reason().is_none()
    }

    pub fn remote_compaction_unavailable_reason(&self) -> Option<&'static str> {
        if self.protocol() != ApiProtocol::OpenAiResponses {
            return Some("protocol is not OpenAI Responses");
        }
        if !self.route.capabilities.remote_compaction {
            return Some("route does not advertise remote compaction");
        }
        if !self.capabilities.remote_compaction {
            return Some("model is not allowlisted for remote compaction");
        }
        None
    }

    pub(crate) fn thinking_level_policy(&self, level: ThinkingLevel) -> ThinkingLevelPolicy<'_> {
        self.capabilities.reasoning.policy(level)
    }

    pub(crate) fn reasoning_has_wire_value(&self, value: &str) -> bool {
        self.capabilities.reasoning.has_wire_value(value)
    }

    pub(crate) fn force_adaptive_thinking(&self) -> bool {
        self.capabilities.reasoning.force_adaptive()
    }
}
