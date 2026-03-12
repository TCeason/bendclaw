use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeRecord {
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub locator: String,
    pub title: String,
    pub summary: String,
    pub metadata: Option<serde_json::Value>,
    pub status: String,
    pub confidence: f64,
    pub user_id: String,
    pub first_run_id: String,
    pub last_run_id: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub created_at: String,
    pub updated_at: String,
}
