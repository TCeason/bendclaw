//! Provider error types and classification.

use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("API error: {0}")]
    Api(String),
    /// A provider-declared transient failure. The original payload is retained
    /// for diagnostics, while retry policy can rely on this semantic variant
    /// instead of matching provider-specific message text.
    #[error("API error: {0}")]
    Transient(String),
    #[error("Overloaded: {0}")]
    Overloaded(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("{}", display_rate_limited(*.retry_after_ms))]
    RateLimited { retry_after_ms: Option<u64> },
    #[error("Context overflow: {message}")]
    ContextOverflow { message: String },
    #[error("Cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

fn display_rate_limited(retry_after_ms: Option<u64>) -> String {
    match retry_after_ms {
        Some(ms) => format!("Rate limited, retry after {ms}ms"),
        None => "Rate limited".to_string(),
    }
}

impl ProviderError {
    /// Classify an HTTP error response into the appropriate variant.
    pub fn classify(status: u16, message: &str, retry_after_ms: Option<u64>) -> Self {
        if is_context_overflow(status, message) {
            Self::ContextOverflow {
                message: message.to_string(),
            }
        } else if status == 429 && is_quota_exceeded(message) {
            Self::Other(message.to_string())
        } else if status == 429 {
            Self::RateLimited { retry_after_ms }
        } else if status == 529 || is_overloaded_message(message) {
            Self::Overloaded(message.to_string())
        } else if status == 401 || status == 403 {
            Self::Auth(message.to_string())
        } else if status == 400 || status == 404 || status == 405 || status == 422 {
            Self::Other(message.to_string())
        } else {
            Self::Api(message.to_string())
        }
    }

    pub fn is_context_overflow(&self) -> bool {
        matches!(self, Self::ContextOverflow { .. })
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited {
                retry_after_ms: Some(ms),
            } => Some(Duration::from_millis(*ms)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// SSE / eventsource classification
// ---------------------------------------------------------------------------

pub fn classify_sse_error_event(message: &str) -> ProviderError {
    if is_context_overflow_message(message) {
        return ProviderError::ContextOverflow {
            message: message.to_string(),
        };
    }
    if is_overloaded_message(message) {
        return ProviderError::Overloaded(message.to_string());
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(message) {
        if provider_error_type(&value).is_some_and(is_transient_provider_error_type) {
            return ProviderError::Transient(message.to_string());
        }
    }

    ProviderError::Api(message.to_string())
}

/// Extract the provider's semantic error type from common JSON envelopes.
/// Nested `error.type` takes precedence over the outer event type (`"error"`).
pub(crate) fn provider_error_type(value: &serde_json::Value) -> Option<&str> {
    value
        .pointer("/error/type")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("type").and_then(serde_json::Value::as_str))
}

/// Provider-declared transient error types. These are protocol fields, not
/// human-readable messages, so classification remains stable if wording changes.
pub(crate) fn is_transient_provider_error_type(error_type: &str) -> bool {
    matches!(
        error_type,
        "api_error" | "server_error" | "invalid_tool_call"
    )
}

// ---------------------------------------------------------------------------
// Context overflow detection
// ---------------------------------------------------------------------------

/// Substrings that indicate a context-overflow error, across every supported
/// provider. This is the single source of truth for overflow detection — the
/// HTTP classifier, the SSE/JSON error paths, the retry policy, and the
/// compaction trigger all route through [`is_context_overflow_message`] rather
/// than maintaining their own copies.
///
/// Each entry documents the provider whose error wording it matches.
const OVERFLOW_PHRASES: &[&str] = &[
    "prompt is too long",                           // Anthropic (token overflow)
    "request_too_large",                            // Anthropic (HTTP 413 byte-size)
    "request too large",                            // Anthropic / Cerebras variant
    "request exceeds the maximum size",             // Anthropic
    "input is too long",                            // AWS Bedrock
    "exceeds the context window",                   // OpenAI (Completions & Responses)
    "maximum context length",                       // OpenAI / OpenRouter / LiteLLM
    "exceeds the maximum number of tokens allowed", // Google Gemini
    "input token count",                            // Google Gemini
    "maximum prompt length",                        // xAI (Grok)
    "reduce the length of the messages",            // Groq
    "exceeds the maximum allowed input length",     // OpenRouter / Poolside
    "is longer than the model's context length",    // Together AI
    "exceeds the limit of",                         // GitHub Copilot
    "prompt token count of",                        // GitHub Copilot
    "exceeds the available context size",           // llama.cpp
    "greater than the context length",              // LM Studio
    "context window exceeds limit",                 // MiniMax
    "exceeded model token limit",                   // Kimi
    "too large for model with",                     // Mistral
    "model_context_window_exceeded",                // z.ai
    "prompt too long; exceeded",                    // Ollama
    "context length exceeded",                      // Generic
    "context_length_exceeded",                      // Generic (underscore variant)
    "too many tokens",                              // Generic
    "token limit exceeded",                         // Generic
];

/// Substrings that indicate a *non*-overflow error even though they may also
/// contain an overflow phrase. Checked first so transient errors are never
/// misclassified as overflow.
///
/// Example: a throttling message like "Too many tokens, please wait before
/// trying again" matches the `too many tokens` overflow phrase, but is really a
/// rate-limit error that should be retried, not compacted.
const NON_OVERFLOW_PHRASES: &[&str] = &[
    "rate limit",        // Generic rate limiting
    "too many requests", // Generic HTTP 429 style
    "throttl",           // AWS Bedrock / generic throttling
];

/// Whether an error message indicates a context overflow.
///
/// Non-overflow wording (rate limits, throttling) is excluded first so a
/// transient error that happens to contain an overflow phrase is not
/// misclassified.
pub fn is_context_overflow_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    if NON_OVERFLOW_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        return false;
    }
    OVERFLOW_PHRASES.iter().any(|phrase| lower.contains(phrase))
}

fn is_context_overflow(status: u16, message: &str) -> bool {
    if (status == 400 || status == 413) && message.trim().is_empty() {
        return true;
    }
    is_context_overflow_message(message)
}

pub(crate) fn is_overloaded_message(message: &str) -> bool {
    message.to_lowercase().contains("overloaded")
}

fn is_quota_exceeded(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("quota_exceeded")
        || lower.contains("quota exceeded")
        || lower.contains("insufficient_quota")
        || lower.contains("out of budget")
        || lower.contains("available balance")
        || lower.contains("usage limit") // Kimi: "reached your usage limit for this period"
        || lower.contains("billing")
}

// ---------------------------------------------------------------------------
// Transport error formatting
// ---------------------------------------------------------------------------

/// Build a concise, human-readable transport error string.
///
/// reqwest wraps a transport failure in several layers (`error sending request
/// for url (...)` -> `client error (SendRequest)` -> `connection error` -> root
/// cause). Concatenating the whole chain produces a long, repetitive line, and
/// some crates (notably rustls) append a docs.rs manual link to their `Display`
/// output. Users only need the root cause plus the URL, so surface the deepest
/// cause, strip any docs.rs reference, and append the request URL.
pub fn format_transport_detail(error: &dyn std::error::Error, url: Option<&str>) -> String {
    let mut root: &dyn std::error::Error = error;
    while let Some(cause) = root.source() {
        root = cause;
    }

    let mut detail = strip_doc_reference(&root.to_string());
    if detail.is_empty() {
        detail = strip_doc_reference(&error.to_string());
    }

    if let Some(url) = url {
        if !url.is_empty() && !detail.contains(url) {
            detail.push_str(&format!(" (url: {url})"));
        }
    }

    detail
}

/// Strip a trailing docs.rs documentation link that some crates (notably
/// rustls) append to their `Display` output, e.g. "peer closed connection
/// without sending TLS close_notify: https://docs.rs/rustls/latest/...". It is
/// noise for users and only bloats the error line.
fn strip_doc_reference(text: &str) -> String {
    match text.find(": https://docs.rs/") {
        Some(idx) => text[..idx].trim_end().to_string(),
        None => text.to_string(),
    }
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

include!(concat!(env!("OUT_DIR"), "/user_agent.rs"));

static SHARED_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

/// Per-read timeout for streaming responses.
///
/// `read_timeout` resets after every successful read, so it only fires when a
/// connection goes silent — not during long-running work where the provider
/// keeps sending data (Anthropic/OpenAI emit periodic `ping`/delta frames while
/// thinking). This is what recovers a half-open TCP connection after the
/// machine sleeps or loses network: the stalled read fails with a timeout,
/// which `stream_http` maps to [`ProviderError::Network`] and the retry policy
/// then reconnects. Without it a half-open socket can hang for the OS TCP
/// timeout (~2 h on macOS), leaving the UI stuck on "thinking".
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(300);

/// Timeout for the connect phase only (TCP + TLS handshake).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

fn build_client() -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(STREAM_READ_TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(8)
        // TCP keepalive idle/interval/retries. The interval and retry count
        // matter on macOS, whose defaults probe only after ~2 h; an explicit
        // short interval lets the OS surface a dead peer well before that.
        .tcp_keepalive(Duration::from_secs(60))
        .tcp_keepalive_interval(Duration::from_secs(15))
        .tcp_keepalive_retries(3)
        .build()
        .map_err(|e| {
            ProviderError::Other(format!(
                "Failed to build HTTP client: {}",
                format_transport_detail(&e, None)
            ))
        })
}

pub fn new_client() -> Result<reqwest::Client, ProviderError> {
    if let Some(client) = SHARED_CLIENT.get() {
        return Ok(client.clone());
    }
    let client = build_client()?;
    let _ = SHARED_CLIENT.set(client.clone());
    Ok(SHARED_CLIENT.get().cloned().unwrap_or(client))
}
