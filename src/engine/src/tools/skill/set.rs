use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

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

    pub fn format_for_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        String::from(
            "When the user's request matches a skill's description, \
             you MUST invoke the skill tool BEFORE generating any other response. \
             Do not explain, summarize, or ask clarifying questions before activating the skill. \
             Never mention a skill without actually calling the skill tool.",
        )
    }
}

pub fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .take_while(|(i, _)| *i < max_chars)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max_chars);
    format!("{}\u{2026}", &s[..end])
}
