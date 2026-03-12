use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::ListQuery;
use crate::service::v1::common::Paginated;
use crate::storage::LearningRecord;

#[derive(Serialize)]
pub struct LearningResponse {
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
    pub source_run_id: String,
    pub success_count: i32,
    pub failure_count: i32,
    pub last_applied_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn to_response(r: LearningRecord) -> LearningResponse {
    LearningResponse {
        id: r.id,
        kind: r.kind,
        subject: r.subject,
        title: r.title,
        content: r.content,
        conditions: r.conditions,
        strategy: r.strategy,
        priority: r.priority,
        confidence: r.confidence,
        status: r.status,
        supersedes_id: r.supersedes_id,
        user_id: r.user_id,
        source_run_id: r.source_run_id,
        success_count: r.success_count,
        failure_count: r.failure_count,
        last_applied_at: r.last_applied_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

#[derive(Deserialize)]
pub struct CreateLearningRequest {
    pub kind: String,
    pub content: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub conditions: Option<serde_json::Value>,
    #[serde(default)]
    pub strategy: Option<serde_json::Value>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub supersedes_id: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchLearningRequest {
    pub query: String,
    pub limit: Option<u32>,
}

pub async fn create_learning(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateLearningRequest>,
) -> Result<Json<LearningResponse>> {
    let record = service::create_learning(&state, &ctx.user_id, &agent_id, req).await?;
    Ok(Json(to_response(record)))
}

pub async fn list_learnings(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<LearningResponse>>> {
    let (records, total) = service::list_learnings(&state, &agent_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(to_response).collect(),
        &q,
        total,
    )))
}

pub async fn get_learning(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, learning_id)): Path<(String, String)>,
) -> Result<Json<LearningResponse>> {
    let record = service::get_learning(&state, &agent_id, &learning_id).await?;
    Ok(Json(to_response(record)))
}

pub async fn search_learnings(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<SearchLearningRequest>,
) -> Result<Json<Vec<LearningResponse>>> {
    let records = service::search_learnings(&state, &agent_id, &req.query, req.limit).await?;
    Ok(Json(records.into_iter().map(to_response).collect()))
}

pub async fn delete_learning(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, learning_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let deleted = service::delete_learning(&state, &agent_id, &learning_id).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
