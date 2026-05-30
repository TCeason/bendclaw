//! Skill tool — activate named skills that provide specialized capabilities.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use crate::types::*;

// ─── SkillSpec + SkillSet ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSpec {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub base_dir: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct SkillSet {
    skills: Vec<SkillSpec>,
}

impl SkillSet {
    /// Deduplicates by name (last wins) and sorts alphabetically.
    pub fn new(skills: Vec<SkillSpec>) -> Self {
        let mut by_name: HashMap<String, SkillSpec> = HashMap::new();
        for skill in skills {
            by_name.insert(skill.name.clone(), skill);
        }
        let mut skills: Vec<SkillSpec> = by_name.into_values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Self { skills }
    }

    pub fn empty() -> Self {
        Self { skills: Vec::new() }
    }

    pub fn merge(&mut self, other: SkillSet) {
        let mut by_name: HashMap<String, SkillSpec> =
            self.skills.drain(..).map(|s| (s.name.clone(), s)).collect();
        for skill in other.skills {
            by_name.insert(skill.name.clone(), skill);
        }
        self.skills = by_name.into_values().collect();
        self.skills.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn find(&self, name: &str) -> Option<&SkillSpec> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn specs(&self) -> &[SkillSpec] {
        &self.skills
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

pub fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let end = s.floor_char_boundary(max_chars);
    format!("{}\u{2026}", &s[..end])
}

// ─── SkillTool ────────────────────────────────────────────────────────────

pub struct SkillTool {
    skills: Arc<SkillSet>,
    description: String,
}

impl SkillTool {
    const MAX_DESC_CHARS: usize = 250;

    pub fn new(skills: Arc<SkillSet>) -> Self {
        let mut desc = String::from(
            "Activate a skill by name. Skills provide specialized capabilities and domain knowledge.\n\n\
             When the user's request matches an available skill, this is a BLOCKING REQUIREMENT: \
             invoke this tool BEFORE generating any other response. \
             NEVER mention a skill without actually calling this tool.\n\n\
             Available skills:\n",
        );
        for skill in skills.specs() {
            let truncated = truncate_str(&skill.description, Self::MAX_DESC_CHARS);
            desc.push_str(&format!("- {}: {}\n", skill.name, truncated));
        }
        Self {
            skills,
            description: desc,
        }
    }
}

fn normalize_name(name: &str) -> &str {
    name.strip_prefix('/').unwrap_or(name)
}

#[async_trait::async_trait]
impl AgentTool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }

    fn label(&self) -> &str {
        "Skill"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "Name of the skill to activate"
                }
            },
            "required": ["skill_name"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let name = normalize_name(params.get("skill_name").and_then(|v| v.as_str())?);
        match self.skills.find(name) {
            Some(skill) if !skill.base_dir.as_os_str().is_empty() => Some(format!(
                "loading skill: {} ({})",
                name,
                skill.base_dir.display()
            )),
            _ => Some(format!("loading skill: {name}")),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let raw_name = params
            .get("skill_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'skill_name' parameter".into()))?;

        let name = normalize_name(raw_name);

        let skill = self.skills.find(name).ok_or_else(|| {
            let available: Vec<&str> = self
                .skills
                .specs()
                .iter()
                .map(|s| s.name.as_str())
                .collect();
            ToolError::Failed(format!(
                "Unknown skill: {name}. Available skills: {}",
                available.join(", ")
            ))
        })?;

        let base_dir_hint = if skill.base_dir.as_os_str().is_empty() {
            String::new()
        } else {
            format!(
                "All relative paths in this skill (e.g. scripts/...) \
                 must be resolved against: {}\n\n",
                skill.base_dir.display(),
            )
        };

        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!(
                    "Activated skill: {name}\n\
                     {base_dir_hint}\
                     Follow the instructions below.\n\n\
                     ---\n{instructions}",
                    instructions = skill.instructions,
                ),
            }],
            details: serde_json::json!({ "skill": name }),
            retention: Retention::CurrentRun,
        })
    }
}
