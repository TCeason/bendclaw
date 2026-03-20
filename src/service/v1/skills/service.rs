use super::http::CreateSkillRequest;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillScope;
use crate::kernel::skills::skill::SkillSource;
use crate::service::error::Result;
use crate::service::state::AppState;

pub(super) async fn list_skills(state: &AppState, agent_id: &str) -> Result<Vec<Skill>> {
    Ok(state.runtime.skills().for_agent(agent_id))
}

pub(super) async fn get_skill(
    state: &AppState,
    agent_id: &str,
    skill_name: &str,
) -> Result<Option<Skill>> {
    Ok(state.runtime.skills().get(agent_id, skill_name))
}

pub(super) async fn create_skill(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    req: CreateSkillRequest,
) -> Result<Skill> {
    let skill = Skill {
        name: req.name,
        version: req.version,
        scope: SkillScope::Agent,
        source: SkillSource::Agent,
        agent_id: Some(agent_id.to_string()),
        created_by: Some(user_id.to_string()),
        description: req.description,
        content: req.content,
        timeout: req.timeout,
        executable: req.executable,
        parameters: req.parameters,
        files: req.files,
        requires: req.requires,
        manifest: req.manifest,
    };
    skill.validate()?;
    state.runtime.create_skill(agent_id, skill.clone()).await?;
    Ok(skill)
}

pub(super) async fn delete_skill(
    state: &AppState,
    agent_id: &str,
    skill_name: &str,
) -> Result<String> {
    state.runtime.delete_skill(agent_id, skill_name).await?;
    Ok(skill_name.to_string())
}
