//! Wiremock-based mock server runners for provider integration tests.

use evotengine::provider::error::ProviderError;
use evotengine::provider::model::ModelConfig;
use evotengine::provider::traits::*;
use evotengine::provider::StreamProvider;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

use super::stream_config::collect_stream_events;

/// Run a provider against a wiremock server returning SSE events.
/// Returns (Message, Vec<StreamEvent>) or ProviderError.
pub async fn run_provider_sse(
    provider: &dyn StreamProvider,
    config: StreamConfig,
    sse_body: &str,
    status: u16,
) -> Result<(Message, Vec<StreamEvent>), ProviderError> {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status)
                .insert_header("content-type", "text/event-stream")
                .insert_header("cache-control", "no-cache")
                .set_body_raw(sse_body.to_string(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let config = override_base_url(config, &server.uri());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let result = provider.stream(config, tx, cancel).await;
    let events = collect_stream_events(&mut rx);

    result.map(|msg| (msg, events))
}

/// Run a provider against a wiremock server returning JSON.
pub async fn run_provider_json(
    provider: &dyn StreamProvider,
    config: StreamConfig,
    json_body: &str,
    status: u16,
) -> Result<(Message, Vec<StreamEvent>), ProviderError> {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status).set_body_raw(json_body.to_string(), "application/json"),
        )
        .mount(&server)
        .await;

    let config = override_base_url(config, &server.uri());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let result = provider.stream(config, tx, cancel).await;
    let events = collect_stream_events(&mut rx);

    result.map(|msg| (msg, events))
}

/// Override the base_url in a StreamConfig's model_config to point at the mock server.
/// For Anthropic (no model_config), creates one with the given base_url.
fn override_base_url(mut config: StreamConfig, base_url: &str) -> StreamConfig {
    let mc = config
        .model_config
        .get_or_insert_with(|| ModelConfig::anthropic("test-model", "Test Model"));
    mc.base_url = base_url.to_string();
    config
}
