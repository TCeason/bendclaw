//! GetVariable tool — retrieve a configured variable by name via host callback.
//!
//! The tool itself is pure logic: it validates parameters, calls the injected
//! `GetVariableFn` callback, and formats the response for the LLM. All variable
//! storage and scope resolution lives in the app layer.

use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;

use crate::types::*;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Response returned by the host callback.
#[derive(Debug, Clone)]
pub enum GetVariableResponse {
    /// Variable was found — contains the value.
    Found(String),
    /// Variable was not found in any scope.
    NotFound,
}

/// Host-provided callback that resolves a variable by name.
/// The app layer implements this — handles scope resolution, last_used tracking,
/// and persistence.
pub type GetVariableFn =
    Arc<dyn Fn(String) -> BoxFuture<'static, Result<GetVariableResponse, String>> + Send + Sync>;

// ---------------------------------------------------------------------------
// Tool
// ---------------------------------------------------------------------------

pub struct GetVariableTool {
    get_fn: GetVariableFn,
}

impl GetVariableTool {
    pub fn new(get_fn: GetVariableFn) -> Self {
        Self { get_fn }
    }
}

#[async_trait]
impl AgentTool for GetVariableTool {
    fn name(&self) -> &str {
        "get_variable"
    }

    fn label(&self) -> &str {
        "Get Variable"
    }

    fn description(&self) -> &str {
        "Retrieve a configured variable by name. Use this when a task or skill \
         explicitly references a variable. Do not guess variable names and do not \
         fetch variables unless required for the current task."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Variable name to retrieve"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'name'".into()))?;

        if name.trim().is_empty() {
            return Err(ToolError::InvalidArgs("'name' must not be empty".into()));
        }

        let response = (self.get_fn)(name.to_string())
            .await
            .map_err(ToolError::Failed)?;

        let text = match response {
            GetVariableResponse::Found(value) => value,
            GetVariableResponse::NotFound => {
                format!("Variable '{}' is not set.", name)
            }
        };

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::Value::Null,
        })
    }
}
