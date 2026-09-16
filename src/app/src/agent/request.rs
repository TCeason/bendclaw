use std::path::PathBuf;

use super::tools::HostTools;
use super::tools::ToolMode;
use super::Run;
use crate::conf::LlmConfig;
use crate::error::EvotError;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct ExecutionLimits {
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub max_duration_secs: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 512,
            max_total_tokens: 100_000_000,
            max_duration_secs: 3600,
        }
    }
}

pub struct QueryRequest {
    pub input: Vec<evot_engine::Content>,
    pub session_id: Option<String>,
    pub mode: ToolMode,
    pub source: String,
    pub llm: Option<LlmConfig>,
    pub host_tools: Option<HostTools>,
    pub cwd: Option<String>,
}

impl QueryRequest {
    pub fn text(prompt: impl Into<String>) -> Self {
        Self {
            input: vec![evot_engine::Content::Text {
                text: prompt.into(),
            }],
            session_id: None,
            mode: ToolMode::Headless,
            source: String::new(),
            llm: None,
            host_tools: None,
            cwd: None,
        }
    }

    pub fn with_input(input: Vec<evot_engine::Content>) -> Self {
        Self {
            input,
            session_id: None,
            mode: ToolMode::Headless,
            source: String::new(),
            llm: None,
            host_tools: None,
            cwd: None,
        }
    }

    pub fn input_text(&self) -> String {
        crate::conversation::convert::extract_content_text(&self.input)
    }

    pub fn session_id(mut self, id: Option<String>) -> Self {
        self.session_id = id;
        self
    }

    pub fn mode(mut self, mode: ToolMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn llm(mut self, llm: LlmConfig) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn host_tools(mut self, host_tools: Option<HostTools>) -> Self {
        self.host_tools = host_tools;
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

pub enum SubmitOutcome {
    Run(Run),
    Command(String),
}

pub(super) fn expand_prompt_command(
    mut request: QueryRequest,
    skills_dirs: &[PathBuf],
) -> Result<QueryRequest> {
    use crate::command::clip_session_prompt;
    use crate::command::parse_command;
    use crate::command::Command;

    if !matches!(
        parse_command(&request.input_text()),
        Some(Command::ClipSession)
    ) {
        return Ok(request);
    }
    let memory = crate::agent::prompt::skill::load_skill(skills_dirs, "memory")
        .map_err(|error| EvotError::Agent(format!("cannot load memory skill: {error}")))?;
    let instructions = crate::agent::prompt::skill::load_skill_instructions(&memory)
        .map_err(|error| EvotError::Agent(format!("cannot read memory skill: {error}")))?;
    let text = clip_session_prompt(&instructions);
    request.input = vec![evot_engine::Content::Text { text }];
    Ok(request)
}
