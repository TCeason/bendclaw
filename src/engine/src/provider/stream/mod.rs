//! Shared streaming transport infrastructure for HTTP providers.
//!
//! `sse` frames the wire format, `http` drives the response body, `sink` is the
//! outbound channel to the agent loop, and `fallback` handles degraded
//! non-streaming responses.

pub mod fallback;
pub mod http;
pub mod sink;
pub mod sse;
