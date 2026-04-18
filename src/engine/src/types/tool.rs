use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use super::message::Content;
use super::message::Retention;
use crate::tools::guard::PathGuard;

// ---------------------------------------------------------------------------
// Tool definition
// ---------------------------------------------------------------------------

/// Callback for streaming partial results during tool execution.
///
/// Tools call this to emit progress updates (e.g., partial output, status messages)
/// that are forwarded as `AgentEvent::ToolExecutionUpdate` events for UI consumption.
/// Partial results are **not** sent to the LLM — only the final `ToolResult` is.
pub type ToolUpdateFn = Arc<dyn Fn(ToolResult) + Send + Sync>;

/// Callback for emitting user-facing progress messages during tool execution.
///
/// Each invocation emits an `AgentEvent::ProgressMessage` event. Unlike `ToolUpdateFn`,
/// these are simple text messages intended for user-facing display (e.g., status lines,
/// notifications), not structured tool results.
pub type ProgressFn = Arc<dyn Fn(String) + Send + Sync>;

/// Context passed to tool execution. Bundles all per-invocation state.
///
/// Using a struct instead of individual parameters future-proofs the trait —
/// adding fields to `ToolContext` is non-breaking.
pub struct ToolContext {
    /// The ID of this tool call (for correlation).
    pub tool_call_id: String,
    /// The name of the tool being invoked.
    pub tool_name: String,
    /// Cancellation token — check `is_cancelled()` in long-running tools.
    pub cancel: tokio_util::sync::CancellationToken,
    /// Optional callback for streaming partial `ToolResult`s (UI/logging only).
    pub on_update: Option<ToolUpdateFn>,
    /// Optional callback for emitting user-facing progress messages.
    pub on_progress: Option<ProgressFn>,
    /// Working directory for path resolution.
    pub cwd: PathBuf,
    /// Path access guard — restricts file tools to allowed directories.
    pub path_guard: Arc<PathGuard>,
}

impl Clone for ToolContext {
    fn clone(&self) -> Self {
        Self {
            tool_call_id: self.tool_call_id.clone(),
            tool_name: self.tool_name.clone(),
            cancel: self.cancel.clone(),
            on_update: self.on_update.clone(),
            on_progress: self.on_progress.clone(),
            cwd: self.cwd.clone(),
            path_guard: self.path_guard.clone(),
        }
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("cancel", &self.cancel)
            .field("on_update", &self.on_update.as_ref().map(|_| "<callback>"))
            .field(
                "on_progress",
                &self.on_progress.as_ref().map(|_| "<callback>"),
            )
            .field("cwd", &self.cwd)
            .field("path_guard", &self.path_guard)
            .finish()
    }
}

/// A tool the agent can call. Implement this trait for your tools.
#[async_trait::async_trait]
pub trait AgentTool: Send + Sync {
    /// Unique tool name (used in LLM tool_use)
    fn name(&self) -> &str;
    /// Human-readable label for UI
    fn label(&self) -> &str;
    /// Description for the LLM
    fn description(&self) -> &str;
    /// JSON Schema for parameters
    fn parameters_schema(&self) -> serde_json::Value;
    /// Preview the system command that will be executed, if applicable.
    ///
    /// Returns `None` for tools that don't invoke external commands.
    /// Used by the UI to display the full command being run.
    fn preview_command(&self, _params: &serde_json::Value) -> Option<String> {
        None
    }

    /// Whether this tool call can safely execute concurrently with other tools.
    /// When a batch contains any tool where this returns `false`,
    /// the entire batch is executed sequentially.
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    /// Execute the tool.
    ///
    /// The `ctx` parameter provides per-invocation context:
    /// - `ctx.tool_call_id` / `ctx.tool_name` — for correlation and logging
    /// - `ctx.cancel` — cancellation token; check `is_cancelled()` in long-running tools
    /// - `ctx.on_update` — optional callback for streaming partial `ToolResult`s (UI/logging only)
    /// - `ctx.on_progress` — optional callback for user-facing progress text (`ProgressMessage`)
    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: Vec<Content>,
    #[serde(default)]
    pub details: serde_json::Value,
    #[serde(default)]
    pub retention: Retention,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Failed(String),
    #[error("Tool not found: {0}")]
    NotFound(String),
    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("Cancelled")]
    Cancelled,
}
