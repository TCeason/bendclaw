use std::collections::HashMap;

use super::capabilities::InputModality;
use super::capabilities::ModelCapabilities;
use super::capabilities::ThinkingLevelPolicy;
use super::capabilities::Verbosity;
use super::overrides::ModelOverrides;
use crate::provider::compat::is_native_openai_responses_route;
use crate::provider::compat::CompatCaps;
use crate::provider::compat::OpenAiCompat;
use crate::provider::protocol::ApiProtocol;
use crate::ThinkingLevel;

/// Features implemented by the selected transport route.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RouteCapabilities {
    pub verbosity: bool,
    pub remote_compaction: bool,
}

impl RouteCapabilities {
    /// Compose endpoint-native route support with explicit compatibility
    /// overrides. Model support is intersected separately by `ModelConfig`.
    pub fn for_route(
        protocol: ApiProtocol,
        provider: &str,
        base_url: &str,
        explicit: CompatCaps,
    ) -> Self {
        let native_responses = is_native_openai_responses_route(provider, base_url);
        Self {
            verbosity: native_responses || explicit.contains(CompatCaps::VERBOSITY),
            remote_compaction: protocol == ApiProtocol::OpenAiResponses
                && (native_responses || explicit.contains(CompatCaps::REMOTE_COMPACTION)),
        }
    }
}

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
        Self::resolve(ResolveModelRequest {
            protocol: ApiProtocol::AnthropicMessages,
            provider: "anthropic".into(),
            model_id: id.into(),
            base_url: "https://api.anthropic.com".into(),
            headers: HashMap::new(),
            compat: None,
            route_capabilities: RouteCapabilities::default(),
            overrides: ModelOverrides::default(),
        })
    }

    pub fn openai(id: impl Into<String>, _display_name: impl Into<String>) -> Self {
        Self::resolve(ResolveModelRequest {
            protocol: ApiProtocol::OpenAiCompletions,
            provider: "openai".into(),
            model_id: id.into(),
            base_url: "https://api.openai.com/v1".into(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::openai()),
            route_capabilities: RouteCapabilities {
                verbosity: true,
                remote_compaction: false,
            },
            overrides: ModelOverrides::default(),
        })
    }

    pub fn openai_responses(id: impl Into<String>, _display_name: impl Into<String>) -> Self {
        Self::resolve(ResolveModelRequest {
            protocol: ApiProtocol::OpenAiResponses,
            provider: "openai".into(),
            model_id: id.into(),
            base_url: "https://api.openai.com/v1".into(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::openai()),
            route_capabilities: RouteCapabilities {
                verbosity: true,
                remote_compaction: true,
            },
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
