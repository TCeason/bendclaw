mod builder;
mod dynamic;
pub mod skill;

pub(crate) use builder::bind_workspace_sections;
pub use builder::Section;
pub use builder::SystemPrompt;
pub use dynamic::dynamic_sections;
pub(crate) use dynamic::prompt_mode;
pub use dynamic::DynamicContext;
pub use dynamic::PromptMode;
pub use skill::format_skills_for_prompt;
pub(crate) use skill::load_turn_skills;
pub(crate) use skill::skills_prompt_section;
pub use skill::SkillSpec;
