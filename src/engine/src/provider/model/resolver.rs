use super::capabilities::ModelCapabilities;
use super::catalog;
use super::resolved::ModelConfig;
use super::resolved::ModelRoute;
use super::resolved::ResolveModelRequest;
use crate::provider::compat::is_official_openai_route;
use crate::provider::protocol::ApiProtocol;
use crate::ThinkingLevel;

pub(super) fn resolve(request: ResolveModelRequest) -> ModelConfig {
    let normalized_id = catalog::normalize_model_id(&request.model_id);
    let family_id = normalized_id
        .strip_prefix("moonshotai/")
        .unwrap_or(&normalized_id);
    let mut capabilities = catalog::resolve(&request.model_id).unwrap_or_else(|| {
        catalog::protocol_fallback(matches!(
            request.protocol,
            ApiProtocol::AnthropicMessages | ApiProtocol::BedrockConverseStream
        ))
    });

    apply_route_rules(
        &mut capabilities,
        request.protocol,
        &request.provider,
        &request.base_url,
        &normalized_id,
        family_id,
    );
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

fn apply_route_rules(
    capabilities: &mut ModelCapabilities,
    protocol: ApiProtocol,
    provider: &str,
    base_url: &str,
    normalized_id: &str,
    family_id: &str,
) {
    if matches!(family_id, "k3" | "kimi-k3") {
        capabilities
            .reasoning
            .replace_level_map(catalog::kimi_k3_thinking_level_map());
    }
    let official_openai = is_official_openai_route(provider, base_url);
    if official_openai && normalized_id == "gpt-5.5" {
        capabilities
            .reasoning
            .insert_override(ThinkingLevel::Minimal, None);
    }
    if protocol == ApiProtocol::OpenAiResponses
        && official_openai
        && supports_openai_none_reasoning(normalized_id)
    {
        capabilities
            .reasoning
            .insert_override(ThinkingLevel::Off, Some("none".into()));
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
