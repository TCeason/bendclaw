//! TodoWrite — always-available task tracking tool.
//! When an active goal exists, delegates to GoalCoordinator.
//! Otherwise maintains session-level task state for self-tracking.

use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use evot_engine::Content;
use evot_engine::Retention;
use evot_engine::ToolContext;
use evot_engine::ToolError;
use evot_engine::ToolResult;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;

use super::GoalCoordinator;
use crate::agent::session::Session;
use crate::types::GoalStatus;
use crate::types::GoalTask;
use crate::types::GoalTaskStatus;

pub type TodoState = Arc<Mutex<Vec<GoalTask>>>;

/// Tracks TodoWrite usage for reminder injection.
#[derive(Clone, Default)]
pub struct TodoMeta {
    pub state: TodoState,
    /// Incremented each turn by the agent.
    pub turn_counter: Arc<AtomicU32>,
    /// Set to current turn_counter value each time TodoWrite is called.
    pub last_used_turn: Arc<AtomicU32>,
}

impl TodoMeta {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Vec::new())),
            turn_counter: Arc::new(AtomicU32::new(0)),
            last_used_turn: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn increment_turn(&self) {
        self.turn_counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn mark_used(&self) {
        let current = self.turn_counter.load(Ordering::Relaxed);
        self.last_used_turn.store(current, Ordering::Relaxed);
    }

    /// Returns true if TodoWrite has never been used and enough turns have passed.
    pub fn should_remind_never_used(&self, threshold: u32) -> bool {
        let current = self.turn_counter.load(Ordering::Relaxed);
        let last = self.last_used_turn.load(Ordering::Relaxed);
        last == 0 && current >= threshold
    }

    /// Returns true if TodoWrite was used before but not recently.
    pub fn should_remind_stale(&self, threshold: u32) -> bool {
        let current = self.turn_counter.load(Ordering::Relaxed);
        let last = self.last_used_turn.load(Ordering::Relaxed);
        last > 0 && current.saturating_sub(last) >= threshold
    }
}

pub struct TodoWriteTool {
    session: Arc<Session>,
    meta: TodoMeta,
}

#[derive(Debug, Deserialize)]
struct TodoWriteRequest {
    tasks: Vec<TaskInput>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TaskInput {
    id: u32,
    content: String,
    status: GoalTaskStatus,
}

impl TodoWriteTool {
    pub fn new(session: Arc<Session>, meta: TodoMeta) -> Self {
        Self { session, meta }
    }

    async fn has_active_goal(&self) -> bool {
        self.session
            .read_goal()
            .await
            .is_some_and(|g| g.status == GoalStatus::Active)
    }
}

#[async_trait]
impl evot_engine::AgentTool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }

    fn label(&self) -> &str {
        "Update Tasks"
    }

    fn description(&self) -> &str {
        "Use this tool to create and manage a structured task list for your current coding session. This helps you track progress, organize complex tasks, and demonstrate thoroughness to the user. It also helps the user understand the progress of the task and overall progress of their requests. Provide the complete current task list each time (full replacement)."
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
                            "id": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Stable numeric task id."
                            },
                            "content": {
                                "type": "string",
                                "description": "Short imperative description of the task."
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"],
                                "description": "Task status. At most one task may be in_progress."
                            }
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
            .filter(|t| t.status == GoalTaskStatus::InProgress)
            .count();
        if in_progress_count > 1 {
            return Err(ToolError::InvalidArgs(
                "At most one task may be in_progress at a time.".into(),
            ));
        }

        let tasks: Vec<GoalTask> = request
            .tasks
            .iter()
            .map(|t| GoalTask::new(t.id, t.content.trim().to_string(), t.status))
            .collect();

        // If active goal exists, delegate to GoalCoordinator for persistence.
        if self.has_active_goal().await {
            let summary = GoalCoordinator::update_tasks(&self.session, tasks.clone())
                .await
                .map_err(|err| ToolError::Failed(err.to_string()))?;

            let mut state = self.meta.state.lock().await;
            *state = tasks;
            self.meta.mark_used();

            return Ok(ToolResult {
                content: vec![Content::Text {
                    text: summary.headline(),
                }],
                details: serde_json::json!({
                    "tasks": summary.tasks,
                    "completed": summary.completed,
                    "total": summary.total,
                }),
                retention: Retention::Normal,
            });
        }

        // No active goal — session-level tracking only.
        let completed = tasks
            .iter()
            .filter(|t| t.status == GoalTaskStatus::Completed)
            .count();
        let total = tasks.len();
        let current = tasks
            .iter()
            .find(|t| t.status == GoalTaskStatus::InProgress)
            .map(|t| format!("#{} {}", t.id, t.title));

        let mut state = self.meta.state.lock().await;
        *state = tasks;
        self.meta.mark_used();

        let headline = match &current {
            Some(task) => format!("Tasks updated ({completed}/{total} done). Working on: {task}"),
            None => format!("Tasks updated ({completed}/{total} done)."),
        };

        Ok(ToolResult {
            content: vec![Content::Text { text: headline }],
            details: serde_json::Value::Null,
            retention: Retention::Normal,
        })
    }
}
