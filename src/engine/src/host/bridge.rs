//! The [`HostBridge`] trait: the engine's single outbound channel to the host.
//!
//! Everything the engine needs to delegate back to the host (currently: tool
//! execution) goes through this one trait. It generalizes the bespoke
//! ask-user callback that previously existed only for the ask_user tool —
//! now any host-registered tool routes through the same path.
//!
//! The app/addon layer provides the concrete implementation (e.g. a NAPI
//! bridge that forwards calls to the TypeScript CLI over a channel). The
//! engine stays agnostic to transport.

use std::sync::Arc;

use futures::future::BoxFuture;

use super::protocol::HostToolCall;
use super::protocol::HostToolResponse;

/// Errors returned when a host delegation fails at the transport level.
///
/// This is distinct from a tool *returning* an error result: a tool that runs
/// and reports failure still yields `Ok(HostToolResponse { is_error: true })`.
/// `HostError` is for the channel itself breaking (host gone, cancelled, etc).
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("host channel closed")]
    Closed,
    #[error("host delegation cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

/// The engine's outbound interface to the host process.
///
/// Implementations must be cheap to clone (wrap shared state in `Arc`) and
/// safe to call from the async agent loop.
#[async_trait::async_trait]
pub trait HostBridge: Send + Sync {
    /// Execute a host-owned tool and return its result.
    ///
    /// Called by [`super::HostTool`] during the agent loop's tool-execution
    /// phase. The host is responsible for running the tool (which may involve
    /// blocking on user input) and returning the result.
    async fn execute_tool(&self, call: HostToolCall) -> Result<HostToolResponse, HostError>;
}

/// Shared handle to a host bridge, held by the agent loop config and tools.
pub type SharedHost = Arc<dyn HostBridge>;

/// Boxed future alias for bridge implementations that build closures.
pub type HostFuture<'a, T> = BoxFuture<'a, Result<T, HostError>>;
