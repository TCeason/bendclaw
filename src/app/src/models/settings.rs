use crate::agent::Agent;
use crate::conf::Config;
use crate::models::ModelSelection;

/// Change the live selection, then best-effort persist the default for future
/// sessions. Failure to save must not undo a successful live selection change.
pub fn cycle_thinking_level(agent: &Agent, env_file: &str) -> Option<String> {
    let level = agent.cycle_thinking_level()?;
    persist_default(agent, env_file, level);
    Some(ModelSelection::display_thinking_level_for(&agent.llm()))
}

/// Reject unsupported levels without changing either the live or saved state.
pub fn set_thinking_level(agent: &Agent, env_file: &str, level: &str) -> Option<String> {
    let parsed = crate::conf::thinking_level_from_str(level).ok()?;
    if !agent.supported_thinking_levels().contains(&parsed) {
        return None;
    }
    agent.set_thinking_level(parsed);
    persist_default(agent, env_file, parsed);
    Some(ModelSelection::display_thinking_level_for(&agent.llm()))
}

fn persist_default(agent: &Agent, env_file: &str, level: evot_engine::ThinkingLevel) {
    if let Ok(mut config) = Config::load_with_env_file(Some(env_file)) {
        let provider = agent.llm().provider.clone();
        let _ = crate::conf::persist_default_thinking_level(&mut config, &provider, level);
    }
}
