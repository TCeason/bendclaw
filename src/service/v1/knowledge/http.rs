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
use crate::storage::dal::knowledge::KnowledgeRecord;

#[derive(Serialize)]
pub struct KnowledgeResponse {
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
    pub scope: String,
    pub created_by: String,
    pub first_run_id: String,
    pub last_run_id: String,
    pub created_at: String,
    pub updated_at: String,
}

fn to_response(r: KnowledgeRecord) -> KnowledgeResponse {
    KnowledgeResponse {
        id: r.id,
        kind: r.kind,
        subject: r.subject,
        locator: r.locator,
        title: r.title,
        summary: r.summary,
        metadata: r.metadata,
        status: r.status,
        confidence: r.confidence,
        user_id: r.user_id,
        scope: r.scope,
        created_by: r.created_by,
        first_run_id: r.first_run_id,
        last_run_id: r.last_run_id,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

#[derive(Deserialize)]
pub struct CreateKnowledgeRequest {
    pub kind: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub locator: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

#[derive(Deserialize)]
pub struct SearchKnowledgeRequest {
    pub query: String,
    pub limit: Option<u32>,
}

pub async fn create_knowledge(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateKnowledgeRequest>,
) -> Result<Json<KnowledgeResponse>> {
    let record = service::create_knowledge(&state, &ctx.user_id, &agent_id, req).await?;
    Ok(Json(to_response(record)))
}

pub async fn list_knowledge(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<KnowledgeResponse>>> {
    let (records, total) = service::list_knowledge(&state, &agent_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(to_response).collect(),
        &q,
        total,
    )))
}

pub async fn get_knowledge(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, knowledge_id)): Path<(String, String)>,
) -> Result<Json<KnowledgeResponse>> {
    let record = service::get_knowledge(&state, &agent_id, &knowledge_id).await?;
    Ok(Json(to_response(record)))
}

pub async fn delete_knowledge(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, knowledge_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    service::delete_knowledge(&state, &agent_id, &knowledge_id).await?;
    Ok(Json(serde_json::json!({ "deleted": knowledge_id })))
}

pub async fn search_knowledge(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<SearchKnowledgeRequest>,
) -> Result<Json<Vec<KnowledgeResponse>>> {
    let records = service::search_knowledge(&state, &agent_id, &req.query, req.limit).await?;
    Ok(Json(records.into_iter().map(to_response).collect()))
}
