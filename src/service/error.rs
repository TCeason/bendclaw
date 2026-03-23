use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde_json::json;

use crate::observability::log::slog;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("rate limited: {0}")]
    RateLimited(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<crate::base::ErrorCode> for ServiceError {
    fn from(e: crate::base::ErrorCode) -> Self {
        use crate::base::ErrorCode;

        match e.code {
            ErrorCode::NOT_FOUND | ErrorCode::SKILL_NOT_FOUND => {
                slog!(warn, "service", "not_found", code = e.code, name = e.name, error = %e,);
                Self::AgentNotFound(e.message)
            }
            ErrorCode::INVALID_INPUT
            | ErrorCode::SKILL_VALIDATION
            | ErrorCode::SKILL_REQUIREMENTS => {
                slog!(warn, "service", "bad_request", code = e.code, name = e.name, error = %e,);
                Self::BadRequest(e.message)
            }
            ErrorCode::DENIED => {
                slog!(warn, "service", "forbidden", code = e.code, name = e.name, error = %e,);
                Self::Forbidden(e.message)
            }
            ErrorCode::LLM_RATE_LIMIT | ErrorCode::QUOTA_EXCEEDED => {
                slog!(warn, "service", "rate_limited", code = e.code, name = e.name, error = %e,);
                Self::RateLimited(e.message)
            }
            _ => {
                let display = format!("[{}] {}: {}", e.code, e.name, e.message);
                slog!(error, "service", "internal_error",
                    code = e.code,
                    name = e.name,
                    error = %e,
                    span_trace = %tracing_error::SpanTrace::capture(),
                );
                Self::Internal(display)
            }
        }
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            Self::AgentNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            Self::BadRequest(m) => {
                slog!(warn, "service", "bad_request", error = %m,);
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, self.to_string()),
            Self::Conflict(_) => (StatusCode::CONFLICT, self.to_string()),
            Self::RateLimited(_) => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ServiceError>;
