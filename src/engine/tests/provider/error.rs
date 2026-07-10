use evotengine::provider::error::*;

#[test]
fn classify_anthropic_overflow() {
    let err = ProviderError::classify(
        400,
        "prompt is too long: 213462 tokens > 200000 maximum",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_openai_overflow() {
    let err = ProviderError::classify(
        400,
        "Your input exceeds the context window of this model",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_google_overflow() {
    let err = ProviderError::classify(
        400,
        "The input token count (1196265) exceeds the maximum number of tokens allowed",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_bedrock_overflow() {
    let err = ProviderError::classify(400, "input is too long for requested model", None);
    assert!(err.is_context_overflow());
}

#[test]
fn classify_xai_overflow() {
    let err = ProviderError::classify(
        400,
        "This model's maximum prompt length is 131072 but request contains 537812 tokens",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_groq_overflow() {
    let err = ProviderError::classify(
        400,
        "Please reduce the length of the messages or completion",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_empty_body_overflow() {
    let err = ProviderError::classify(413, "", None);
    assert!(err.is_context_overflow());
    let err = ProviderError::classify(400, "  ", None);
    assert!(err.is_context_overflow());
}

#[test]
fn classify_rate_limit() {
    let err = ProviderError::classify(429, "rate limit exceeded", None);
    assert!(matches!(err, ProviderError::RateLimited { .. }));
}

#[test]
fn classify_rate_limit_with_retry_after() {
    let err = ProviderError::classify(429, "rate limit exceeded", Some(5000));
    match err {
        ProviderError::RateLimited { retry_after_ms } => {
            assert_eq!(retry_after_ms, Some(5000));
        }
        _ => panic!("Expected RateLimited"),
    }
}

#[test]
fn classify_quota_exhausted_429_is_not_retryable() {
    // A 429 that signals exhausted quota (vs. transient rate limiting) must not
    // retry: the quota only refreshes next billing period, so retrying just
    // hammers the same error. Kimi's wording is the regression case here.
    let kimi = "rate_limit_error: You've reached your usage limit for this period. \
        Your quota will be refreshed in the next period. Upgrade to get more.";
    let err = ProviderError::classify(429, kimi, None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));

    // Other quota phrasings stay non-retryable too.
    for msg in ["insufficient_quota", "quota exceeded", "out of budget"] {
        let err = ProviderError::classify(429, msg, None);
        assert!(matches!(err, ProviderError::Other(_)), "{msg}");
        assert!(!evotengine::retry::should_retry(&err), "{msg}");
    }
}

#[test]
fn retry_policy_default_matches_claude_style_backoff_budget() {
    let policy = evotengine::RetryPolicy::default();
    assert_eq!(policy.max_retries(), 10);

    let first = policy.delay_for_attempt(1).as_millis();
    let second = policy.delay_for_attempt(2).as_millis();
    let late = policy.delay_for_attempt(10).as_millis();

    assert!((1600..=2400).contains(&first));
    assert!((3200..=4800).contains(&second));
    assert!((24000..=36000).contains(&late));
}

#[test]
fn classify_auth_error() {
    let err = ProviderError::classify(401, "invalid api key", None);
    assert!(matches!(err, ProviderError::Auth(_)));
    assert!(!evotengine::retry::should_retry(&err));
    let err = ProviderError::classify(403, "forbidden", None);
    assert!(matches!(err, ProviderError::Auth(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_400_not_retryable() {
    let err = ProviderError::classify(400, "invalid request format", None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));

    let err = ProviderError::Api("HTTP 400 Bad Request: missing text field".into());
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_529_overloaded() {
    let err = ProviderError::classify(529, "overloaded", None);
    assert!(matches!(err, ProviderError::Overloaded(_)));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_sse_overloaded_error() {
    let err = classify_sse_error_event(r#"{"type":"overloaded_error","message":"Overloaded"}"#);
    assert!(matches!(err, ProviderError::Overloaded(_)));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_overloaded_message_without_status() {
    // Plain-text "overloaded" wording (no 529 status) routes to Overloaded.
    let err = ProviderError::classify(500, "Our servers are currently overloaded", None);
    assert!(matches!(err, ProviderError::Overloaded(_)));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn empty_response_api_error_is_retryable() {
    // Both SSE decoders surface an empty 200 (no content, no usage) as an Api
    // error. It is a transient provider/proxy defect and must retry, matching
    // the Network promotion in the agent loop and pi's retryable stream errors.
    let err = ProviderError::Api("Empty response from provider (no content, no usage)".into());
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn overloaded_api_message_is_retryable() {
    // Even when surfaced as a bare Api error, overloaded wording retries.
    let err = ProviderError::Api(
        "API error: Our servers are currently overloaded. Please try again later.".into(),
    );
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn structured_server_error_with_empty_message_is_retryable() {
    let err = ProviderError::Api(r#"{"error": {"message": "", "type": "server_error"}}"#.into());
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn try_again_later_is_retryable() {
    let err = ProviderError::Api("The model is busy, please try again later.".into());
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn stream_interrupted_api_error_is_retryable() {
    let err = ProviderError::Api(
        r#"{"type":"error","error":{"type":"api_error","message":"Stream interrupted. Please retry."}}"#
            .into(),
    );
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn malformed_tool_call_api_error_is_retryable() {
    // A malformed tool_use JSON error is a transient model-output defect, not a
    // client error: sampling is non-deterministic so re-running usually yields
    // valid JSON. This is common with smaller local models (e.g. qwen3-4b),
    // where the gateway rejects the tool call outright instead of emitting
    // recoverable deltas. Retrying beats surfacing a fatal error.
    let err = ProviderError::Api(
        r#"{"type": "error", "error": {"type": "invalid_tool_call", "message": "malformed tool_use JSON; could not recover a valid tool call"}}"#
            .into(),
    );
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn overflow_message_case_insensitive() {
    assert!(is_context_overflow_message("PROMPT IS TOO LONG"));
    assert!(is_context_overflow_message("Too Many Tokens in request"));
}

#[test]
fn non_overflow_messages() {
    assert!(!is_context_overflow_message("invalid api key"));
    assert!(!is_context_overflow_message("internal server error"));
    assert!(!is_context_overflow_message(""));
}

#[test]
fn classify_404_not_retryable() {
    let err = ProviderError::classify(404, "model not found", None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_405_not_retryable() {
    let err = ProviderError::classify(405, "method not allowed", None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_422_not_retryable() {
    let err = ProviderError::classify(422, "unprocessable entity", None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn overflow_message_with_try_again_is_not_retryable() {
    // Regression: an overflow error whose wording also contains a transient
    // phrase ("try again") must NOT retry — it is handled by compaction.
    let msg = "Your input exceeds the context window of this model. \
               Please adjust your input and try again.";
    assert!(is_context_overflow_message(msg));
    let err = ProviderError::Api(msg.into());
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn throttling_with_too_many_tokens_is_not_overflow() {
    // Bedrock-style throttling contains the "too many tokens" overflow phrase
    // but is a rate-limit error — the non-overflow exclusion must win.
    let msg = "ThrottlingException: Too many tokens, please wait before trying again.";
    assert!(!is_context_overflow_message(msg));
}

#[test]
fn rate_limit_wording_is_not_overflow() {
    assert!(!is_context_overflow_message(
        "Rate limit reached: too many tokens per minute"
    ));
    assert!(!is_context_overflow_message("429 too many requests"));
}

// ---------------------------------------------------------------------------
// format_transport_detail
// ---------------------------------------------------------------------------

use std::error::Error;
use std::fmt;

#[derive(Debug)]
struct FakeError {
    msg: String,
    source: Option<Box<FakeError>>,
}

impl fmt::Display for FakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl Error for FakeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn Error + 'static))
    }
}

#[test]
fn transport_detail_appends_url_when_missing() {
    let err = FakeError {
        msg: "connection reset".into(),
        source: None,
    };
    let detail = format_transport_detail(&err, Some("https://example.com/v1/messages"));
    assert_eq!(
        detail,
        "connection reset (url: https://example.com/v1/messages)"
    );
}

#[test]
fn transport_detail_does_not_duplicate_url_already_present() {
    let url = "https://example.com/v1/messages";
    let err = FakeError {
        msg: format!("error sending request for url ({url})"),
        source: None,
    };
    let detail = format_transport_detail(&err, Some(url));
    assert_eq!(detail.matches(url).count(), 1);
}

#[test]
fn transport_detail_skips_repeated_source_text() {
    let inner = FakeError {
        msg: "peer closed connection without sending TLS close_notify".into(),
        source: None,
    };
    let middle = FakeError {
        msg: "peer closed connection without sending TLS close_notify".into(),
        source: Some(Box::new(inner)),
    };
    let outer = FakeError {
        msg: "client error (SendRequest): peer closed connection without sending TLS close_notify"
            .into(),
        source: Some(Box::new(middle)),
    };
    let detail = format_transport_detail(&outer, Some("https://example.com/v1/messages"));
    assert_eq!(
        detail
            .matches("peer closed connection without sending TLS close_notify")
            .count(),
        1
    );
}

#[test]
fn transport_detail_surfaces_root_cause_from_chain() {
    // reqwest nests the real failure at the bottom of the source chain. Surface
    // only the deepest cause instead of concatenating every wrapper layer.
    let inner = FakeError {
        msg: "dns lookup failed".into(),
        source: None,
    };
    let outer = FakeError {
        msg: "connect error".into(),
        source: Some(Box::new(inner)),
    };
    let detail = format_transport_detail(&outer, None);
    assert_eq!(detail, "dns lookup failed");
}

#[test]
fn transport_detail_strips_docs_rs_reference() {
    // rustls appends a docs.rs manual link to its Display output; it is noise
    // for users and must be trimmed.
    let err = FakeError {
        msg: "peer closed connection without sending TLS close_notify: \
              https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof"
            .into(),
        source: None,
    };
    let detail = format_transport_detail(&err, Some("https://example.com/v1/messages"));
    assert_eq!(
        detail,
        "peer closed connection without sending TLS close_notify (url: https://example.com/v1/messages)"
    );
}
