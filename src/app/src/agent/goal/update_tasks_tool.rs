//! UpdateGoalTasksTool — model-facing task progress updates for active goals.

use std::sync::Arc;

use async_trait::async_trait;
use evot_engine::Content;
use evot_engine::Retention;
use evot_engine::ToolContext;
use evot_engine::ToolError;
use evot_engine::ToolResult;
use serde::Deserialize;
use serde::Serialize;

use super::GoalCoordinator;
use crate::agent::session::Session;
use crate::types::GoalTask;
use crate::types::GoalTaskStatus;

pub struct UpdateGoalTasksTool {
    session: Arc<Session>,
}

#[derive(Debug, Deserialize)]
struct UpdateGoalTasksRequest {
    tasks: Vec<GoalTaskInput>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GoalTaskInput {
    id: u32,
    title: String,
    status: GoalTaskStatus,
}

impl UpdateGoalTasksTool {
    pub fn new(session: Arc<Session>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl evot_engine::AgentTool for UpdateGoalTasksTool {
    fn name(&self) -> &str {
        "update_goal_tasks"
    }

    fn label(&self) -> &str {
        "Update Goal Tasks"
    }

    fn description(&self) -> &str {
        "Create or update the active goal's ordered task list. Use this before doing goal work when no tasks exist, then update it after meaningful progress. Provide the complete current task list each time."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "Complete ordered task list for the active goal.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Stable numeric task id."
                            },
                            "title": {
                                "type": "string",
                                "description": "Short, concrete task title."
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"],
                                "description": "Task status. At most one task may be in_progress."
                            }
                        },
                        "required": ["id", "title", "status"]
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
        let request: UpdateGoalTasksRequest = serde_json::from_value(params)
            .map_err(|err| ToolError::InvalidArgs(err.to_string()))?;
        if request.tasks.is_empty() {
            return Err(ToolError::InvalidArgs(
                "tasks must contain at least one task".into(),
            ));
        }

        let tasks: Vec<GoalTask> = request
            .tasks
            .iter()
            .map(|task| GoalTask::new(task.id, task.title.trim().to_string(), task.status))
            .collect();

        let summary = GoalCoordinator::update_tasks(&self.session, tasks)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        Ok(ToolResult {
            content: vec![Content::Text {
                text: summary.headline(),
            }],
            details: serde_json::json!({
                "goal": {
                    "completed": summary.completed,
                    "total": summary.total,
                    "current": summary.current.map(|task| format!("#{} {}", task.id, task.title)),
                    "tasks": summary.tasks,
                }
            }),
            retention: Retention::Normal,
        })
    }
}
