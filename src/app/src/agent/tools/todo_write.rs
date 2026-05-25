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

use super::super::goal::GoalCoordinator;
use crate::agent::session::Session;
use crate::types::GoalStatus;
use crate::types::GoalTask;
use crate::types::GoalTaskStatus;

pub type TodoState = Arc<Mutex<Vec<GoalTask>>>;

const TODO_WRITE_DESCRIPTION: &str = "\
Use this tool to create and manage a structured task list for your current coding session. \
This helps you track progress, organize complex tasks, and demonstrate thoroughness to the user. \
It also helps the user understand the progress of the task and overall progress of their requests. \
Provide the complete current task list each time (full replacement).

## When to Use This Tool
Use this tool proactively in these scenarios:

1. Complex multi-step tasks - When a task requires 3 or more distinct steps or actions
2. Non-trivial and complex tasks - Tasks that require careful planning or multiple operations
3. User explicitly requests todo list - When the user directly asks you to use the todo list
4. User provides multiple tasks - When users provide a list of things to be done (numbered or comma-separated)
5. After receiving new instructions - Immediately capture user requirements as todos
6. When you start working on a task - Mark it as in_progress BEFORE beginning work. Ideally you should only have one todo as in_progress at a time
7. After completing a task - Mark it as completed and add any new follow-up tasks discovered during implementation

## When NOT to Use This Tool

Skip using this tool when:
1. There is only a single, straightforward task
2. The task is trivial and tracking it provides no organizational benefit
3. The task can be completed in less than 3 trivial steps
4. The task is purely conversational or informational

NOTE that you should not use this tool if there is only one trivial task to do. In this case you are better off just doing the task directly.

## Examples of When to Use the Todo List

<example>
User: I want to add a dark mode toggle to the application settings. Make sure you run the tests and build when you're done!
Assistant: *Creates todo list with the following items:*
1. Creating dark mode toggle component in Settings page
2. Adding dark mode state management (context/store)
3. Implementing CSS-in-JS styles for dark theme
4. Updating existing components to support theme switching
5. Running tests and build process, addressing any failures or errors that occur
*Begins working on the first task*

<reasoning>
The assistant used the todo list because:
1. Adding dark mode is a multi-step feature requiring UI, state management, and styling changes
2. The user explicitly requested tests and build be run afterward
3. The assistant inferred that tests and build need to pass by adding the final task
</reasoning>
</example>

<example>
User: Help me rename the function getCwd to getCurrentWorkingDirectory across my project
Assistant: *Uses grep or search tools to locate all instances of getCwd in the codebase*
I've found 15 instances of 'getCwd' across 8 different files.
*Creates todo list with specific items for each file that needs updating*

<reasoning>
The assistant used the todo list because:
1. First, the assistant searched to understand the scope of the task
2. Upon finding multiple occurrences across different files, it determined this was a complex task
3. The todo list helps ensure every instance is tracked and updated systematically
</reasoning>
</example>

<example>
User: I need to implement these features for my e-commerce site: user registration, product catalog, shopping cart, and checkout flow.
Assistant: *Creates a todo list breaking down each feature into specific tasks based on the project architecture*

<reasoning>
The assistant used the todo list because:
1. The user provided multiple complex features to implement in a comma separated list
2. The todo list helps organize these large features into manageable tasks
3. This approach allows for tracking progress across the entire implementation
</reasoning>
</example>

<example>
User: Can you help optimize my React application? It's rendering slowly and has performance issues.
Assistant: *Reviews component structure, render patterns, state management, and data fetching*
*Creates todo list with items like: 1) Implementing memoization, 2) Adding virtualization for long lists, 3) Optimizing image loading, 4) Fixing state update loops, 5) Reviewing bundle size and code splitting*

<reasoning>
The assistant used the todo list because:
1. First, the assistant examined the codebase to identify specific performance issues
2. Based on this analysis, it identified multiple optimization opportunities
3. Performance optimization is a non-trivial task requiring multiple steps
</reasoning>
</example>

## Examples of When NOT to Use the Todo List

<example>
User: How do I print 'Hello World' in Python?
Assistant: print(\"Hello World\")

<reasoning>
Did not use the todo list because this is a single, trivial task.
</reasoning>
</example>

<example>
User: What does the git status command do?
Assistant: [explains git status]

<reasoning>
Did not use the todo list because this is an informational request with no coding task.
</reasoning>
</example>

<example>
User: Can you add a comment to the calculateTotal function?
Assistant: [adds the comment directly]

<reasoning>
Did not use the todo list because this is a single, straightforward task confined to one location.
</reasoning>
</example>

## Task States and Management

1. **Task States**: Use these states to track progress:
   - pending: Task not yet started
   - in_progress: Currently working on (limit to ONE task at a time)
   - completed: Task finished successfully

   **IMPORTANT**: Task descriptions must have two forms:
   - content: The imperative form describing what needs to be done (e.g., \"Run tests\", \"Build the project\")
   - activeForm: The present continuous form shown during execution (e.g., \"Running tests\", \"Building the project\")

2. **Task Management**:
   - Update task status in real-time as you work
   - Mark tasks complete IMMEDIATELY after finishing (don't batch completions)
   - Exactly ONE task must be in_progress at any time (not less, not more)
   - Complete current tasks before starting new ones
   - Remove tasks that are no longer relevant from the list entirely

3. **Task Completion Requirements**:
   - ONLY mark a task as completed when you have FULLY accomplished it
   - If you encounter errors, blockers, or cannot finish, keep the task as in_progress
   - When blocked, create a new task describing what needs to be resolved
   - Never mark a task as completed if:
     - Tests are failing
     - Implementation is partial
     - You encountered unresolved errors
     - You couldn't find necessary files or dependencies

4. **Task Breakdown**:
   - Create specific, actionable items
   - Break complex tasks into smaller, manageable steps
   - Use clear, descriptive task names
   - Always provide both forms:
     - content: \"Fix authentication bug\"
     - activeForm: \"Fixing authentication bug\"

When in doubt, use this tool. Being proactive with task management demonstrates attentiveness and ensures you complete all requirements successfully.

## Efficiency

Combine TodoWrite with other tool calls in the same response when possible. Do not use a separate turn just to update task status.";

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
    #[serde(default, rename = "activeForm")]
    active_form: Option<String>,
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
                            },
                            "activeForm": {
                                "type": "string",
                                "description": "Present continuous form shown during execution (e.g., \"Running tests\")."
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
