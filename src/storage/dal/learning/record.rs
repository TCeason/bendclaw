use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningRecord {
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub title: String,
    pub content: String,
    pub conditions: Option<serde_json::Value>,
    pub strategy: Option<serde_json::Value>,
    pub priority: i32,
    pub confidence: f64,
    pub status: String,
    pub supersedes_id: String,
    pub user_id: String,
    pub scope: String,
    pub created_by: String,
    pub source_run_id: String,
    pub success_count: i32,
    pub failure_count: i32,
    pub last_applied_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
