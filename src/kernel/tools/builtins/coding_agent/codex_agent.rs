use std::path::Path;

use tokio::process::Command;

use crate::kernel::tools::cli_agent::AgentEvent;
use crate::kernel::tools::cli_agent::AgentOptions;
use crate::kernel::tools::cli_agent::CliAgent;

pub struct CodexAgent;

impl CliAgent for CodexAgent {
    fn agent_type(&self) -> &str {
        "codex"
    }

    fn build_command(&self, cwd: &Path, prompt: &str, opts: &AgentOptions) -> Command {
        let mut cmd = self.base_command();
        cmd.current_dir(cwd);
        cmd.args([
            "exec",
            "--json",
            "--dangerously-bypass-approvals-and-sandbox",
        ]);
        if let Some(model) = &opts.model {
            cmd.args(["--model", model]);
        }
        cmd.arg(prompt);
        cmd
    }

    fn build_resume_command(
        &self,
        cwd: &Path,
        session_id: &str,
        prompt: &str,
        opts: &AgentOptions,
    ) -> Command {
        let mut cmd = self.base_command();
        cmd.current_dir(cwd);
        cmd.args([
            "exec",
            "resume",
            "--json",
            "--dangerously-bypass-approvals-and-sandbox",
            session_id,
        ]);
        if let Some(model) = &opts.model {
            cmd.args(["--model", model]);
        }
        if !prompt.is_empty() {
            cmd.arg(prompt);
        }
        cmd
    }

    fn parse_session_id(&self, line: &serde_json::Value) -> Option<String> {
        if line.get("type")?.as_str()? == "thread.started" {
            return line.get("thread_id")?.as_str().map(|s| s.to_string());
        }
        None
    }

    fn parse_events(&self, line: &serde_json::Value) -> Vec<AgentEvent> {
        let Some(msg_type) = line.get("type").and_then(|t| t.as_str()) else {
            return vec![];
        };

        match msg_type {
            "item.started" => self.parse_item_started(line),
            "item.completed" => self.parse_item_completed(line),
            "item.reasoning.textDelta" | "item.reasoning.summaryTextDelta" => {
                if let Some(text) = line.get("text").and_then(|t| t.as_str()) {
                    if !text.is_empty() {
                        return vec![AgentEvent::Thinking {
                            content: text.to_string(),
                        }];
                    }
                }
                vec![]
            }
            "item.agentMessage.delta" => {
                if let Some(text) = line.get("text").and_then(|t| t.as_str()) {
                    if !text.is_empty() {
                        return vec![AgentEvent::Text {
                            content: text.to_string(),
                        }];
                    }
                }
                vec![]
            }
            "turn.plan.updated" => {
                let steps = line
                    .get("steps")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                vec![AgentEvent::System {
                    subtype: "plan_updated".to_string(),
                    metadata: serde_json::json!({ "steps": steps }),
                }]
            }
            "error" => {
                let message = line
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                vec![AgentEvent::Error {
                    message: message.to_string(),
                }]
            }
            _ => vec![],
        }
    }

    fn parse_result(&self, line: &serde_json::Value) -> Option<String> {
        if line.get("type")?.as_str()? != "turn.completed" {
            return None;
        }
        Some(line.to_string())
    }
}

impl CodexAgent {
    fn parse_item_started(&self, line: &serde_json::Value) -> Vec<AgentEvent> {
        let Some(item) = line.get("item") else {
            return vec![];
        };
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("");

        match item_type {
            "commandExecution" => {
                let cmd = item.get("command").and_then(|c| c.as_str()).unwrap_or("?");
                vec![AgentEvent::ToolUse {
                    tool_name: "Bash".to_string(),
                    tool_use_id: id.to_string(),
                    input: serde_json::json!({ "command": cmd }),
                }]
            }
            "fileChange" => {
                let filename = item.get("filename").and_then(|f| f.as_str()).unwrap_or("?");
                vec![AgentEvent::ToolUse {
                    tool_name: "FileEdit".to_string(),
                    tool_use_id: id.to_string(),
                    input: serde_json::json!({ "filename": filename }),
                }]
            }
            "mcpToolCall" => {
                let name = item
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("mcp_tool");
                let server = item.get("server").and_then(|s| s.as_str()).unwrap_or("");
                let tool_name = if server.is_empty() {
                    name.to_string()
                } else {
                    format!("{server}::{name}")
                };
                vec![AgentEvent::ToolUse {
                    tool_name,
                    tool_use_id: id.to_string(),
                    input: item
                        .get("arguments")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                }]
            }
            "webSearch" => {
                let query = item.get("query").and_then(|q| q.as_str()).unwrap_or("");
                vec![AgentEvent::ToolUse {
                    tool_name: "WebSearch".to_string(),
                    tool_use_id: id.to_string(),
                    input: serde_json::json!({ "query": query }),
                }]
            }
            _ => vec![],
        }
    }

    fn parse_item_completed(&self, line: &serde_json::Value) -> Vec<AgentEvent> {
        let Some(item) = line.get("item") else {
            return vec![];
        };
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("");

        match item_type {
            "agent_message" | "agentMessage" => {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    if !text.is_empty() {
                        return vec![AgentEvent::Text {
                            content: text.to_string(),
                        }];
                    }
                }
                vec![]
            }
            "commandExecution" | "command_execution" => {
                let exit_code = item.get("exit_code").and_then(|c| c.as_i64());
                let success = exit_code == Some(0);
                let cmd = item.get("command").and_then(|c| c.as_str()).unwrap_or("?");
                let output = format!(
                    "[exec:{cmd} exit:{}]",
                    exit_code.map_or("?".to_string(), |c| c.to_string())
                );
                vec![AgentEvent::ToolResult {
                    tool_use_id: id.to_string(),
                    success,
                    output,
                }]
            }
            "fileChange" => {
                let filename = item.get("filename").and_then(|f| f.as_str()).unwrap_or("?");
                let status = item
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("completed");
                vec![AgentEvent::ToolResult {
                    tool_use_id: id.to_string(),
                    success: status != "failed",
                    output: format!("[file:{filename} status:{status}]"),
                }]
            }
            _ => vec![],
        }
    }
}
