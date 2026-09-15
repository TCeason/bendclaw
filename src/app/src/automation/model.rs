//! Wire DTOs for cloud scheduled tasks.
//!
//! These types are a published contract with the cloud API. Added fields must
//! stay backward-readable, so every non-essential field carries `serde(default)`.

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunSummary {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub delivery_status: String,
    pub scheduled_for: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskStats {
    #[serde(default)]
    pub window_days: i64,
    #[serde(default)]
    pub runs: i64,
    #[serde(default)]
    pub completed: i64,
    #[serde(default)]
    pub succeeded: i64,
    #[serde(default)]
    pub execution_success_rate: Option<f64>,
    #[serde(default)]
    pub delivery_attempted: i64,
    #[serde(default)]
    pub delivery_sent: i64,
    #[serde(default)]
    pub delivery_success_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub revision: i64,
    pub name: String,
    pub cron: String,
    pub timezone: String,
    pub instruction: String,
    pub executor_id: String,
    #[serde(default = "default_model_policy")]
    pub model_policy: String,
    #[serde(default)]
    pub model_spec: String,
    #[serde(default)]
    pub thinking_level: String,
    #[serde(default)]
    pub workspace_ref: String,
    #[serde(default)]
    pub delivery_channel: String,
    #[serde(default)]
    pub delivery_target: String,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: i64,
    #[serde(default = "default_max_lateness_seconds")]
    pub max_lateness_seconds: i64,
    pub enabled: bool,
    pub next_run_at: i64,
    #[serde(default)]
    pub last_run: Option<TaskRunSummary>,
    #[serde(default)]
    pub recent_runs: Vec<TaskRunSummary>,
    #[serde(default)]
    pub stats: TaskStats,
}

fn default_model_policy() -> String {
    "default".to_string()
}

fn default_timeout_seconds() -> i64 {
    900
}

fn default_max_lateness_seconds() -> i64 {
    14_400
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskList {
    pub tasks: Vec<Task>,
    pub cache: CacheStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStatus {
    pub ready: bool,
    pub synced_at: i64,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedTask {
    pub task: Task,
    pub next_runs: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimedRun {
    pub id: String,
    pub task_id: String,
    pub status: String,
    pub scheduled_for: i64,
    pub expires_at: i64,
    pub task_snapshot: Task,
    pub lease_token: String,
}
