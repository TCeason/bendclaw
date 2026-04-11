use bendengine::provider::traits::*;

#[test]
fn classify_anthropic_overflow() {
    let err = ProviderError::classify(400, "prompt is too long: 213462 tokens > 200000 maximum");
    assert!(err.is_context_overflow());
}

#[test]
fn classify_openai_overflow() {
    let err = ProviderError::classify(400, "Your input exceeds the context window of this model");
    assert!(err.is_context_overflow());
}

#[test]
fn classify_google_overflow() {
    let err = ProviderError::classify(
        400,
        "The input token count (1196265) exceeds the maximum number of tokens allowed",
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_bedrock_overflow() {
    let err = ProviderError::classify(400, "input is too long for requested model");
    assert!(err.is_context_overflow());
}

#[test]
fn classify_xai_overflow() {
    let err = ProviderError::classify(
        400,
        "This model's maximum prompt length is 131072 but request contains 537812 tokens",
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_groq_overflow() {
    let err = ProviderError::classify(
        400,
        "Please reduce the length of the messages or completion",
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_empty_body_overflow() {
    let err = ProviderError::classify(413, "");
    assert!(err.is_context_overflow());
    let err = ProviderError::classify(400, "  ");
    assert!(err.is_context_overflow());
}

#[test]
fn classify_rate_limit() {
    let err = ProviderError::classify(429, "rate limit exceeded");
    assert!(matches!(err, ProviderError::RateLimited { .. }));
}

#[test]
fn classify_auth_error() {
    let err = ProviderError::classify(401, "invalid api key");
    assert!(matches!(err, ProviderError::Auth(_)));
    let err = ProviderError::classify(403, "forbidden");
    assert!(matches!(err, ProviderError::Auth(_)));
}

#[test]
fn classify_400_not_retryable() {
    let err = ProviderError::classify(400, "invalid request format");
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!err.is_retryable());
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
    let err = ProviderError::classify(404, "model not found");
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!err.is_retryable());
}

#[test]
fn classify_405_not_retryable() {
    let err = ProviderError::classify(405, "method not allowed");
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!err.is_retryable());
}

#[test]
fn classify_422_not_retryable() {
    let err = ProviderError::classify(422, "unprocessable entity");
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!err.is_retryable());
}
