use axum::extract::Path;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::kernel::skills::manifest::SkillManifest;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillFile;
use crate::kernel::skills::skill::SkillParameter;
use crate::kernel::skills::skill::SkillRequirements;
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;

#[derive(Serialize)]
pub struct SkillResponse {
    pub name: String,
    pub version: String,
    pub scope: String,
    pub source: String,
    pub description: String,
    pub content: String,
    pub timeout: u64,
    pub executable: bool,
    pub created_by_user_id: Option<String>,
    pub parameters: Vec<SkillParameter>,
    pub files: Vec<SkillFile>,
    pub requires: Option<SkillRequirements>,
    pub manifest: Option<SkillManifest>,
}

fn to_response(s: &Skill) -> SkillResponse {
    SkillResponse {
        name: s.name.clone(),
        version: s.version.clone(),
        scope: s.scope.as_str().to_string(),
        source: s.source.as_str().to_string(),
        description: s.description.clone(),
        content: s.content.clone(),
        timeout: s.timeout,
        executable: s.executable,
        created_by_user_id: s.created_by_user_id.clone(),
        parameters: s.parameters.clone(),
        files: s.files.clone(),
        requires: s.requires.clone(),
        manifest: s.manifest.clone(),
    }
}

#[derive(Deserialize)]
pub struct CreateSkillRequest {
    pub name: String,
    pub description: String,
    pub content: String,
    #[serde(default)]
    pub executable: bool,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub parameters: Vec<SkillParameter>,
    #[serde(default)]
    pub files: Vec<SkillFile>,
    #[serde(default)]
    pub requires: Option<SkillRequirements>,
    #[serde(default)]
    pub manifest: Option<SkillManifest>,
}

fn default_version() -> String {
    "0.0.1".to_string()
}

fn default_timeout() -> u64 {
    30
}

pub async fn list_skills(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<SkillResponse>>> {
    let skills = service::list_skills(&state, &agent_id).await?;
    Ok(Json(skills.iter().map(to_response).collect()))
}

pub async fn get_skill(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, skill_name)): Path<(String, String)>,
) -> Result<Json<SkillResponse>> {
    let skill = service::get_skill(&state, &agent_id, &skill_name)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("skill '{skill_name}' not found")))?;
    Ok(Json(to_response(&skill)))
}

pub async fn create_skill(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateSkillRequest>,
) -> Result<Json<SkillResponse>> {
    let skill = service::create_skill(&state, &ctx.user_id, &agent_id, req).await?;
    Ok(Json(to_response(&skill)))
}

pub async fn delete_skill(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, skill_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let deleted = service::delete_skill(&state, &agent_id, &skill_name).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
