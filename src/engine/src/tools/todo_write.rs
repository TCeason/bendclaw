//! TodoWrite — engine-level task tracking tool.
//! Maintains task state in-memory; injected into context after compaction.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::types::*;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoTask {
    pub id: u32,
    pub content: String,
    pub status: TodoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
}

/// Shared task state. Cheap to clone (Arc).
pub type TodoState = Arc<Mutex<Vec<TodoTask>>>;

/// Create a new empty TodoState.
pub fn new_todo_state() -> TodoState {
    Arc::new(Mutex::new(Vec::new()))
}

/// Format tasks for injection after compaction. Returns None if empty.
pub fn format_todo_for_compaction(state: &TodoState) -> Option<String> {
    let tasks = state.try_lock().ok()?;
    if tasks.is_empty() {
        return None;
    }
    let mut lines = Vec::with_capacity(tasks.len() + 4);
    lines.push("# Current tasks".to_string());
    lines.push(String::new());
    lines.push("These tasks are already tracked. Only call TodoWrite to change status (e.g. mark completed), not to recreate this list.".to_string());
    lines.push(String::new());
    for t in tasks.iter() {
        let status_str = match t.status {
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
            TodoStatus::Pending => "pending",
        };
        lines.push(format!("- [{}] {}", status_str, t.content));
    }
    Some(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Tool description
// ---------------------------------------------------------------------------

const TODO_WRITE_DESCRIPTION: &str = "\
Track progress for multi-step tasks (3+ steps). Provide the complete task list each time (full replacement). \
Only call at key milestones: initial plan, status change, or completion. \
Always combine with other tool calls in the same response — never use a separate turn just to update tasks. \
At most one task may be in_progress at a time. \
Each task needs: id (integer), content (imperative), status (pending|in_progress|completed), and activeForm (present continuous).";

// ---------------------------------------------------------------------------
// Tool implementation
// ---------------------------------------------------------------------------

pub struct TodoWriteTool {
    state: TodoState,
}

#[derive(Debug, Deserialize)]
struct TodoWriteRequest {
    tasks: Vec<TaskInput>,
}

#[derive(Debug, Deserialize)]
struct TaskInput {
    id: u32,
    content: String,
    status: TodoStatus,
    #[serde(default, rename = "activeForm")]
    active_form: Option<String>,
}

impl TodoWriteTool {
    pub fn new(state: TodoState) -> Self {
        Self { state }
    }
}
#[async_trait]
impl crate::AgentTool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }

    fn label(&self) -> &str {
        "Update Tasks"
    }

    fn description(&self) -> &str {
        TODO_WRITE_DESCRIPTION
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "Complete ordered task list. Replaces the existing list entirely.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "minimum": 1, "description": "Stable numeric task id." },
                            "content": { "type": "string", "description": "Short imperative description of the task." },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed"], "description": "Task status. At most one task may be in_progress." },
                            "activeForm": { "type": "string", "description": "Present continuous form shown during execution (e.g., \"Running tests\")." }
                        },
                        "required": ["id", "content", "status"]
                    }
                }
            },
            "required": ["tasks"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let request: TodoWriteRequest = serde_json::from_value(params)
            .map_err(|err| ToolError::InvalidArgs(err.to_string()))?;

        let in_progress_count = request
            .tasks
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        if in_progress_count > 1 {
            return Err(ToolError::InvalidArgs(
                "At most one task may be in_progress at a time.".into(),
            ));
        }
        let tasks: Vec<TodoTask> = request
            .tasks
            .into_iter()
            .map(|t| TodoTask {
                id: t.id,
                content: t.content.trim().to_string(),
                status: t.status,
                active_form: t.active_form,
            })
            .collect();

        // No-op detection
        {
            let current = self.state.lock().await;
            if tasks_equal(&current, &tasks) {
                return Ok(ToolResult {
                    content: vec![Content::Text {
                        text: "Tasks unchanged.".into(),
                    }],
                    details: serde_json::Value::Null,
                    retention: Retention::CurrentRun,
                });
            }
        }

        let completed = tasks
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        let total = tasks.len();
        let current_task = tasks
            .iter()
            .find(|t| t.status == TodoStatus::InProgress)
            .map(|t| t.active_form.as_deref().unwrap_or(&t.content).to_string());

        let mut state = self.state.lock().await;
        *state = tasks;

        let headline = match &current_task {
            Some(task) => format!("Tasks updated ({completed}/{total} done). Working on: {task}"),
            None => format!("Tasks updated ({completed}/{total} done)."),
        };

        Ok(ToolResult {
            content: vec![Content::Text { text: headline }],
            details: serde_json::Value::Null,
            retention: Retention::CurrentRun,
        })
    }
}

fn tasks_equal(current: &[TodoTask], incoming: &[TodoTask]) -> bool {
    if current.len() != incoming.len() {
        return false;
    }
    current
        .iter()
        .zip(incoming.iter())
        .all(|(a, b)| a.id == b.id && a.content == b.content && a.status == b.status)
}
