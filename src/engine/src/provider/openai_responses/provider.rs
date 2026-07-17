//! OpenAI Responses API provider.

use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::debug;

use super::request;
use super::sse_decode;
use crate::provider::error::ProviderError;
use crate::provider::stream_http;
use crate::provider::traits::*;

pub struct OpenAiResponsesProvider;

#[async_trait]
impl StreamProvider for OpenAiResponsesProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let model_config = config.model_config.as_ref().ok_or_else(|| {
            ProviderError::Other("ModelConfig required for OpenAI Responses provider".into())
        })?;
        let url = format!("{}/responses", model_config.base_url.trim_end_matches('/'));
        let body = request::build_request_body(&config);
        debug!(
            "OpenAI Responses request: model={} url={}",
            config.model, url
        );

        let client = crate::provider::error::new_client()?;
        let mut builder = client
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", config.api_key));
        for (key, value) in &model_config.headers {
            builder = builder.header(key, value);
        }

        let response = stream_http::send_stream_request(builder.json(&body)).await?;
        let response = stream_http::check_error_status(response).await?;
        match stream_http::classify_response(&response) {
            stream_http::StreamResponseKind::Streaming => {
                sse_decode::decode_sse_stream(response, tx, cancel, &config).await
            }
            stream_http::StreamResponseKind::Json => Err(ProviderError::Api(
                "OpenAI Responses returned JSON for a streaming request".into(),
            )),
            stream_http::StreamResponseKind::Other(content_type) => Err(ProviderError::Api(
                format!("Unexpected content type from OpenAI Responses endpoint: {content_type}"),
            )),
        }
    }
}
