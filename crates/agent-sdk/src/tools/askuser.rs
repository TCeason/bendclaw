use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use serde_json::Value;

use crate::types::Tool;
use crate::types::ToolError;
use crate::types::ToolInputSchema;
use crate::types::ToolResult;
use crate::types::ToolUseContext;

/// Callback function for asking the user a question.
pub type AskUserFn =
    Arc<dyn Fn(&str) -> futures::future::BoxFuture<'static, Result<String, String>> + Send + Sync>;

#[derive(Default)]
pub struct AskUserTool {
    ask_fn: Option<AskUserFn>,
}

impl AskUserTool {
    pub fn new(ask_fn: AskUserFn) -> Self {
        Self {
            ask_fn: Some(ask_fn),
        }
    }
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }

    fn description(&self) -> &str {
        "Ask the user a question and wait for their response. Use when you need clarification or input."
    }

    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            schema_type: "object".to_string(),
            properties: HashMap::from([(
                "question".to_string(),
                json!({
                    "type": "string",
                    "description": "The question to ask the user"
                }),
            )]),
            required: vec!["question".to_string()],
            additional_properties: Some(false),
        }
    }

    async fn call(&self, input: Value, _context: &ToolUseContext) -> Result<ToolResult, ToolError> {
        let question = input
            .get("question")
            .and_then(|q| q.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing 'question' field".to_string()))?;

        match &self.ask_fn {
            Some(ask_fn) => {
                let answer = (ask_fn)(question)
                    .await
                    .map_err(ToolError::ExecutionError)?;
                Ok(ToolResult::text(answer))
            }
            None => Ok(ToolResult::error(
                "User interaction is not configured. Provide an ask_fn when creating the agent."
                    .to_string(),
            )),
        }
    }
}
