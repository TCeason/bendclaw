//! OpenAPI tool adapter — auto-generate `AgentTool` implementations from OpenAPI specs.
//!
//! Parses an OpenAPI 3.0 spec and creates one tool per operation. The agent can
//! then call any API endpoint as a tool, with parameters validated by JSON Schema.
//!
//! # Example
//!
//! ```rust,no_run
//! use bendengine::openapi::OpenApiConfig;
//! use bendengine::openapi::OpenApiToolAdapter;
//! use bendengine::openapi::OperationFilter;
//!
//! # fn example() -> Result<(), bendengine::openapi::OpenApiError> {
//! let spec = std::fs::read_to_string("openapi.yaml")?;
//! let config = OpenApiConfig::new().with_bearer_token("sk-...");
//! let tools = OpenApiToolAdapter::from_str(&spec, config, &OperationFilter::All)?;
//! // Each tool implements AgentTool — add via Agent::with_openapi_spec() or similar
//! # Ok(())
//! # }
//! ```

pub mod adapter;
pub mod types;

pub use adapter::OpenApiToolAdapter;
pub use types::OpenApiAuth;
pub use types::OpenApiConfig;
pub use types::OpenApiError;
pub use types::OperationFilter;
