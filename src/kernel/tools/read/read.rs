use async_trait::async_trait;

use crate::base::Result;
use crate::kernel::tools::execution::context::ToolContext;
use crate::kernel::tools::execution::id::ToolId;
use crate::kernel::tools::execution::tool::OperationClassifier;
use crate::kernel::tools::execution::tool::Tool;
use crate::kernel::tools::execution::tool::ToolResult;
use crate::kernel::OpType;

/// Read file contents from the session workspace.
pub struct FileReadTool;

impl FileReadTool {
    fn extract_path(args: &serde_json::Value) -> &str {
        args.get("path").and_then(|v| v.as_str()).unwrap_or("")
    }
}

impl OperationClassifier for FileReadTool {
    fn op_type(&self) -> OpType {
        OpType::FileRead
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        Self::extract_path(args).to_string()
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        ToolId::Read.as_str()
    }

    fn description(&self) -> &str {
        super::prompt::DESCRIPTION
    }

    fn parameters_schema(&self) -> serde_json::Value {
        super::schema::schema()
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Missing 'path' parameter")),
        };

        let full_path = match ctx.workspace.resolve_safe_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path escapes workspace directory")),
        };

        match tokio::fs::read_to_string(&full_path).await {
            Ok(contents) => {
                tracing::info!(path, size_bytes = contents.len(), "file read succeeded");
                Ok(ToolResult::ok(contents))
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "file read failed");
                Ok(ToolResult::error(format!("Failed to read file: {e}")))
            }
        }
    }
}
