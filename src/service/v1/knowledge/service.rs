use super::http::CreateKnowledgeRequest;
use crate::base::new_id;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;
use crate::storage::dal::knowledge::KnowledgeRecord;
use crate::storage::dal::knowledge::KnowledgeRepo;

pub(super) async fn list_knowledge(
    state: &AppState,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<KnowledgeRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = KnowledgeRepo::new(pool.clone());
    let limit = q.limit();
    let records = repo.list(limit).await?;
    let total = count_u64(&pool, "SELECT COUNT(*) FROM knowledge").await;
    Ok((records, total))
}

pub(super) async fn create_knowledge(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    req: CreateKnowledgeRequest,
) -> Result<KnowledgeRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = KnowledgeRepo::new(pool);
    let record = KnowledgeRecord {
        id: new_id(),
        kind: req.kind,
        subject: req.subject.unwrap_or_default(),
        locator: req.locator.unwrap_or_default(),
        title: req.title.unwrap_or_default(),
        summary: req.summary.unwrap_or_default(),
        metadata: req.metadata,
        status: req.status.unwrap_or_else(|| "active".to_string()),
        confidence: req.confidence.unwrap_or(1.0),
        user_id: user_id.to_string(),
        scope: "shared".to_string(),
        created_by: String::new(),
        first_run_id: String::new(),
        last_run_id: String::new(),
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

pub(super) async fn get_knowledge(
    state: &AppState,
    agent_id: &str,
    knowledge_id: &str,
) -> Result<KnowledgeRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = KnowledgeRepo::new(pool);
    repo.get(knowledge_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("knowledge not found: {knowledge_id}")))
}

pub(super) async fn delete_knowledge(
    state: &AppState,
    agent_id: &str,
    knowledge_id: &str,
) -> Result<()> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = KnowledgeRepo::new(pool);
    repo.get(knowledge_id).await?.ok_or_else(|| {
        ServiceError::AgentNotFound(format!("knowledge not found: {knowledge_id}"))
    })?;
    repo.delete(knowledge_id).await?;
    Ok(())
}

pub(super) async fn search_knowledge(
    state: &AppState,
    agent_id: &str,
    query: &str,
    limit: Option<u32>,
) -> Result<Vec<KnowledgeRecord>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = KnowledgeRepo::new(pool);
    let limit = limit.unwrap_or(10).min(100);
    Ok(repo.search(query, limit).await?)
}
