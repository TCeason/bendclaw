use super::http::CreateLearningRequest;
use crate::base::new_id;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;
use crate::storage::dal::learning::repo::LearningRepo;
use crate::storage::LearningRecord;

pub(super) async fn list_learnings(
    state: &AppState,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<LearningRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = LearningRepo::new(pool.clone());
    let limit = q.limit();
    let records = repo.list(limit).await?;
    let total = count_u64(&pool, "SELECT COUNT(*) FROM learnings").await;
    Ok((records, total))
}

pub(super) async fn search_learnings(
    state: &AppState,
    agent_id: &str,
    query: &str,
    limit: Option<u32>,
) -> Result<Vec<LearningRecord>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = LearningRepo::new(pool);
    let limit = limit.unwrap_or(10).min(100);
    Ok(repo.search(query, limit).await?)
}

pub(super) async fn create_learning(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    req: CreateLearningRequest,
) -> Result<LearningRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = LearningRepo::new(pool);
    let record = LearningRecord {
        id: new_id(),
        kind: req.kind,
        subject: req.subject.unwrap_or_default(),
        title: req.title.unwrap_or_default(),
        content: req.content,
        conditions: req.conditions,
        strategy: req.strategy,
        priority: req.priority.unwrap_or(0),
        confidence: req.confidence.unwrap_or(1.0),
        status: "active".to_string(),
        supersedes_id: req.supersedes_id.unwrap_or_default(),
        user_id: user_id.to_string(),
        source_run_id: String::new(),
        success_count: 0,
        failure_count: 0,
        last_applied_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    repo.insert(&record).await?;
    // Re-fetch to get server-generated timestamps
    match repo.get(&record.id).await? {
        Some(r) => Ok(r),
        None => Ok(record),
    }
}

pub(super) async fn get_learning(
    state: &AppState,
    agent_id: &str,
    learning_id: &str,
) -> Result<LearningRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = LearningRepo::new(pool);
    repo.get(learning_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("learning not found: {learning_id}")))
}

pub(super) async fn delete_learning(
    state: &AppState,
    agent_id: &str,
    learning_id: &str,
) -> Result<String> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = LearningRepo::new(pool);
    repo.get(learning_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("learning not found: {learning_id}")))?;
    repo.delete(learning_id).await?;
    Ok(learning_id.to_string())
}
