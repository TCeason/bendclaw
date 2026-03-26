use crate::kernel::skills::manifest::CredentialSpec;
use crate::kernel::skills::skill::Skill;
use crate::service::state::AppState;

pub(super) fn list_hub_skills(state: &AppState) -> Vec<Skill> {
    state
        .runtime
        .skills()
        .loaded_skills()
        .into_iter()
        .filter(|s| s.skill.source == crate::kernel::skills::skill::SkillSource::Hub)
        .map(|s| s.skill)
        .collect()
}

pub(super) fn hub_status(state: &AppState) -> HubStatus {
    let store = state.runtime.skills();
    let hub_config = store.hub_config().cloned();
    let last_sync = crate::kernel::skills::hub::sync::last_sync_time(store.workspace_root());
    let hub_skills: Vec<_> = store
        .loaded_skills()
        .into_iter()
        .filter(|s| s.skill.source == crate::kernel::skills::skill::SkillSource::Hub)
        .collect();
    HubStatus {
        enabled: hub_config.is_some(),
        repo_url: hub_config
            .as_ref()
            .map(|c| c.repo_url.clone())
            .unwrap_or_default(),
        skill_count: hub_skills.len(),
        last_sync_epoch: last_sync
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
    }
}

pub(super) fn skill_credentials(state: &AppState, skill_name: &str) -> Vec<CredentialSpec> {
    let skill = state.runtime.skills().get_hub(skill_name);
    skill
        .and_then(|s| s.manifest)
        .map(|m| m.credentials)
        .unwrap_or_default()
}

pub struct HubStatus {
    pub enabled: bool,
    pub repo_url: String,
    pub skill_count: usize,
    pub last_sync_epoch: Option<u64>,
}
