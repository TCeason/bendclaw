//! Tests for shared stream HTTP helpers.

use evotengine::provider::stream_http::check_error_status;
use evotengine::provider::stream_http::classify_json_error;
use evotengine::provider::stream_http::extract_json_error_message;
use evotengine::provider::stream_http::StreamResponseKind;
use evotengine::provider::ProviderError;

// ---------------------------------------------------------------------------
// extract_json_error_message
// ---------------------------------------------------------------------------

#[test]
fn extract_anthropic_error_message() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "overloaded_error",
            "message": "Overloaded"
        }
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("overloaded_error: Overloaded".into()));
}

#[test]
fn extract_openai_error_message() {
    let value = serde_json::json!({
        "error": {
            "message": "server error"
        }
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("server error".into()));
}

#[test]
fn extract_generic_message_field() {
    let value = serde_json::json!({
        "message": "internal error"
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("internal error".into()));
}

#[test]
fn extract_type_only() {
    let value = serde_json::json!({
        "type": "rate_limit_error"
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("rate_limit_error".into()));
}

#[test]
fn extract_no_known_fields() {
    let value = serde_json::json!({"foo": "bar"});
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, None);
}

// ---------------------------------------------------------------------------
// classify_json_error
// ---------------------------------------------------------------------------

#[test]
fn classify_overflow_json() {
    let value = serde_json::json!({
        "error": {
            "type": "invalid_request_error",
            "message": "prompt is too long: 213462 tokens > 200000 maximum"
        }
    });
    let err = classify_json_error(&value);
    assert!(err.is_context_overflow());
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_internal_server_error_json_is_retryable() {
    let value = serde_json::json!({
        "error": {
            "type": "api_error",
            "message": "Internal server error"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Transient { .. }));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_overloaded_json_is_retryable() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "overloaded_error",
            "message": "service is overloaded"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Overloaded(_)));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_no_message_uses_full_json() {
    // No recognizable error fields: the payload still arrived on an accepted
    // (2xx) request, so it defaults to a retryable transient error rather
    // than failing hard on unknown shapes.
    let value = serde_json::json!({"foo": "bar"});
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Transient { .. }));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_json_404_is_not_retryable() {
    let value = serde_json::json!({
        "error": {
            "type": "not_found_error",
            "message": "model not found"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_json_400_bad_request_is_not_retryable() {
    let value = serde_json::json!({
        "error": {
            "type": "invalid_request_error",
            "message": "Bad request: missing required parameter text"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

// ---------------------------------------------------------------------------
// StreamResponseKind (via classify_response — tested indirectly through
// the public enum since classify_response takes a reqwest::Response)
// ---------------------------------------------------------------------------

#[test]
fn stream_response_kind_variants() {
    // Just verify the enum is usable
    let streaming = StreamResponseKind::Streaming;
    let json = StreamResponseKind::Json;
    let other = StreamResponseKind::Other("text/plain".into());

    assert_eq!(streaming, StreamResponseKind::Streaming);
    assert_eq!(json, StreamResponseKind::Json);
    assert!(matches!(other, StreamResponseKind::Other(_)));
}

// ---------------------------------------------------------------------------
// check_error_status — gateway hint headers
// ---------------------------------------------------------------------------

async fn error_from_mock_response(template: wiremock::ResponseTemplate) -> ProviderError {
    use wiremock::matchers::method;
    use wiremock::Mock;
    use wiremock::MockServer;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(template)
        .mount(&server)
        .await;
    let response = match reqwest::get(server.uri()).await {
        Ok(response) => response,
        Err(error) => panic!("mock request failed: {error}"),
    };
    match check_error_status(response).await {
        Err(error) => error,
        Ok(_) => panic!("expected an error classification"),
    }
}

#[tokio::test]
async fn check_error_status_honors_should_retry_and_retry_after() {
    // A gateway marking an unknown 4xx as retryable (x-should-retry: true)
    // with a Retry-After hint must surface as Transient with that delay.
    let err = error_from_mock_response(
        wiremock::ResponseTemplate::new(402)
            .insert_header("x-should-retry", "true")
            .insert_header("retry-after", "7")
            .set_body_string("payment required"),
    )
    .await;
    assert!(matches!(err, ProviderError::Transient { .. }));
    assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(7)));
    assert!(evotengine::retry::should_retry(&err));
}

#[tokio::test]
async fn check_error_status_retry_after_reaches_5xx() {
    let err = error_from_mock_response(
        wiremock::ResponseTemplate::new(503)
            .insert_header("retry-after", "12")
            .set_body_string("maintenance"),
    )
    .await;
    assert!(matches!(err, ProviderError::Transient { .. }));
    assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(12)));
}
