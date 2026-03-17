use serde::Deserialize;
use serde::Serialize;

/// Structured event parsed from a CLI agent's stdout.
/// Provider-agnostic — Claude and Codex adapters map their JSON into this.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AgentEvent {
    Text {
        content: String,
    },
    Thinking {
        content: String,
    },
    ToolUse {
        tool_name: String,
        tool_use_id: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        success: bool,
        output: String,
    },
    System {
        subtype: String,
        #[serde(default)]
        metadata: serde_json::Value,
    },
    Error {
        message: String,
    },
}

impl AgentEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Thinking { .. } => "thinking",
            Self::ToolUse { .. } => "tool_use",
            Self::ToolResult { .. } => "tool_result",
            Self::System { .. } => "system",
            Self::Error { .. } => "error",
        }
    }
}
