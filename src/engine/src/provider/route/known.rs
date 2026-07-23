use super::ApiProtocol;

const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

const NATIVE_OPENAI_RESPONSES_ROUTES: &[&str] = &[
    OPENAI_BASE_URL,
    "https://openrouter.databend.cloud/openai/v1",
];

pub fn default_base_url(protocol: ApiProtocol, provider: &str) -> &'static str {
    match (protocol, provider) {
        (ApiProtocol::AnthropicMessages, _) => ANTHROPIC_BASE_URL,
        (ApiProtocol::OpenAiCompletions | ApiProtocol::OpenAiResponses, "openai") => {
            OPENAI_BASE_URL
        }
        _ => "",
    }
}

/// Whether a route targets OpenAI's canonical first-party API.
pub fn is_official_openai_route(provider: &str, base_url: &str) -> bool {
    provider == "openai" && normalized_base_url(base_url) == OPENAI_BASE_URL
}

/// Whether a route is known to expose OpenAI Responses-native extensions.
pub fn is_native_openai_responses_route(provider: &str, base_url: &str) -> bool {
    provider == "openai" && NATIVE_OPENAI_RESPONSES_ROUTES.contains(&normalized_base_url(base_url))
}

fn normalized_base_url(base_url: &str) -> &str {
    base_url.trim().trim_end_matches('/')
}
