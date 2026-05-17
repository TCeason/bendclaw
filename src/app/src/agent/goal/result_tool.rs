//! GoalResultTool — structured output tool for the goal verifier agent.
//!
//! The verifier sub-agent calls this tool to report whether the goal
//! condition is met.

use std::sync::Arc;

use async_trait::async_trait;
use evot_engine::ToolContext;
use evot_engine::ToolError;
use evot_engine::ToolResult;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct GoalResultCapture {
    pub ok: Option<bool>,
    pub reason: String,
}

pub struct GoalResultTool {
    capture: Arc<Mutex<GoalResultCapture>>,
}

impl GoalResultTool {
    pub fn new(capture: Arc<Mutex<GoalResultCapture>>) -> Self {
        Self { capture }
    }
}

#[async_trait]
impl evot_engine::AgentTool for GoalResultTool {
    fn name(&self) -> &str {
        "goal_result"
    }

    fn label(&self) -> &str {
        "Goal Result"
    }

    fn description(&self) -> &str {
        "Report whether the goal condition has been met. Call this tool exactly once when done verifying."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "ok": {
                    "type": "boolean",
                    "description": "true if the condition is met, false if not met"
                },
                "reason": {
                    "type": "string",
                    "description": "Brief explanation of why the condition is or is not met"
                }
            },
            "required": ["ok"]
        })
    }
    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let ok = params
            .get("ok")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| ToolError::InvalidArgs("'ok' (boolean) is required".into()))?;
        let reason = params
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        {
            let mut cap = self.capture.lock().await;
            cap.ok = Some(ok);
            cap.reason = reason.clone();
        }

        let msg = if ok {
            "Goal condition met.".to_string()
        } else {
            format!("Goal condition not met: {reason}")
        };

        Ok(ToolResult {
            content: vec![evot_engine::Content::Text { text: msg }],
            details: serde_json::Value::Null,
            retention: evot_engine::Retention::default(),
        })
    }
}
