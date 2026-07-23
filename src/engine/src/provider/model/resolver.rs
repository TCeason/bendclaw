use super::catalog;
use super::resolved::ModelConfig;
use super::resolved::ModelRoute;
use super::resolved::ResolveModelRequest;
use crate::provider::route::is_official_openai_route;
use crate::provider::route::ApiProtocol;

pub(super) fn resolve(request: ResolveModelRequest) -> ModelConfig {
    let normalized_id = catalog::normalize_model_id(&request.model_id);
    let mut capabilities = catalog::resolve(&normalized_id).unwrap_or_else(|| {
        catalog::protocol_fallback(matches!(
            request.protocol,
            ApiProtocol::AnthropicMessages | ApiProtocol::BedrockConverseStream
        ))
    });

    if is_official_openai_route(&request.provider, &request.base_url) {
        for (level, effort) in std::mem::take(&mut capabilities.first_party_reasoning_levels) {
            capabilities.reasoning.insert_override(level, effort);
        }
        if request.protocol == ApiProtocol::OpenAiResponses {
            for (level, effort) in
                std::mem::take(&mut capabilities.first_party_responses_reasoning_levels)
            {
                capabilities.reasoning.insert_override(level, effort);
            }
        }
    }
    request.overrides.apply(&mut capabilities);

    ModelConfig {
        id: request.model_id,
        route: ModelRoute {
            protocol: request.protocol,
            provider: request.provider,
            base_url: request.base_url,
            headers: request.headers,
            compat: request.compat,
            capabilities: request.route_capabilities,
        },
        capabilities,
    }
}
