use std::path::Path;
use std::process::Command;

const PROJECT_CONTEXT_FILES: &[&str] = &["EVOT.md", "CLAUDE.md", "AGENTS.md"];
const DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

const SYSTEM_SECTION: &str = r#""#;

const USING_TOOLS_HEADER: &str = "Guidelines:";
const BASH_FILE_OPS_GUIDELINE: &str = "Use bash for file operations like ls, rg, find";
const USING_TOOLS_TRAILER: &[&str] = &[
    "Be concise in your responses",
    "Show file paths clearly when working with files",
];

const IDENTITY_INTRO: &str =
    "You are an expert coding assistant. You help users by reading files, \
     executing commands, editing code, and writing new files.";
const IDENTITY_OUTRO: &str =
    "In addition to the tools above, you may have access to other custom tools depending on the project.";

const TONE_AND_STYLE_SECTION: &str = r#""#;

const OUTPUT_FORMAT_SECTION: &str = r#""#;

const OUTPUT_EFFICIENCY_SECTION: &str = r#""#;

const CLARIFYING_QUESTIONS_SECTION: &str = r#""#;

const CONTEXT_MANAGEMENT_SECTION: &str = r#""#;

const EXECUTING_ACTIONS_SECTION: &str = r#""#;

const AGENT_BEHAVIOR_SECTION: &str = r#""#;

/// Builder for assembling the system prompt.
///
/// ```ignore
/// let prompt = SystemPrompt::new("/path/to/project")
///     .with_system_guidance()
///     .with_agent_behavior()
///     .with_tool_guidance()
///     .with_tone_and_style()
///     .with_output_format()
///     .with_clarifying_questions()
///     .with_output_efficiency()
///     .with_context_management()
///     .with_environment_static()
///     .with_tools()
///     .with_project_context()
///     .with_dynamic_boundary()
///     .with_today_date()
///     .with_git()
///     .with_memory()
///     .with_append("Be concise.")
///     .build();
/// ```
/// A named section of the system prompt.
///
/// Preserved through the builder so callers (including prompt-dump tooling) can
/// see which logical chunk each piece of text came from rather than diffing one
/// big string. The final prompt is just `sections.join("\n\n")`.
#[derive(Debug, Clone)]
pub struct Section {
    pub name: &'static str,
    pub text: String,
}

pub struct SystemPrompt {
    cwd: String,
    has_bash: bool,
    tools_guidelines: Vec<String>,
    sections: Vec<Section>,
}

impl SystemPrompt {
    pub fn new(cwd: &str) -> Self {
        Self::with_tool_set(cwd, &[])
    }

    /// Construct a prompt whose identity "Available tools" list and guidelines
    /// are derived from the given tools, matching the pi harness exactly:
    /// a tool appears in "Available tools" only when it provides a snippet, and
    /// the guidelines section is assembled in tool order with dedup.
    pub fn with_tool_set(cwd: &str, tools: &[Box<dyn evot_engine::AgentTool>]) -> Self {
        let mut text = String::from(IDENTITY_INTRO);
        let listed: Vec<&Box<dyn evot_engine::AgentTool>> = tools
            .iter()
            .filter(|t| t.prompt_snippet().is_some())
            .collect();
        if !listed.is_empty() {
            text.push_str("\n\nAvailable tools:");
            for t in &listed {
                if let Some(snippet) = t.prompt_snippet() {
                    text.push_str(&format!("\n- {}: {}", t.name(), snippet));
                }
            }
            text.push_str("\n\n");
            text.push_str(IDENTITY_OUTRO);
        }
        Self {
            cwd: cwd.to_string(),
            has_bash: tools.iter().any(|t| t.name() == "bash"),
            tools_guidelines: tools
                .iter()
                .flat_map(|t| {
                    t.prompt_guidelines()
                        .into_iter()
                        .map(|g| g.to_string())
                        .collect::<Vec<_>>()
                })
                .collect(),
            sections: vec![Section {
                name: "identity",
                text,
            }],
        }
    }

    /// Append system/runtime guidance: user-visible text, permission mode,
    /// system tags, prompt injection, and context compression.
    pub fn with_system_guidance(mut self) -> Self {
        self.sections.push(Section {
            name: "system",
            text: SYSTEM_SECTION.into(),
        });
        self
    }

    /// Append agent behavior guidelines: task execution, code style, and action bias.
    pub fn with_agent_behavior(mut self) -> Self {
        self.sections.push(Section {
            name: "agent_behavior",
            text: AGENT_BEHAVIOR_SECTION.into(),
        });
        self
    }

    /// Append tool-use guidance assembled from the registered tools' own
    /// `prompt_guidelines`, matching the pi harness ordering exactly:
    /// the conditional bash file-ops line first, then per-tool guidelines in
    /// tool order, then the shared trailer lines — all deduplicated.
    pub fn with_tool_guidance(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        let mut guidelines: Vec<String> = Vec::new();
        let mut add = |g: &str| {
            let g = g.trim();
            if !g.is_empty() && seen.insert(g.to_string()) {
                guidelines.push(g.to_string());
            }
        };

        if self.has_bash {
            add(BASH_FILE_OPS_GUIDELINE);
        }
        for g in &self.tools_guidelines {
            add(g);
        }
        for t in USING_TOOLS_TRAILER {
            add(t);
        }

        let mut lines = vec![USING_TOOLS_HEADER.to_string()];
        lines.extend(guidelines.into_iter().map(|g| format!("- {g}")));
        self.sections.push(Section {
            name: "using_tools",
            text: lines.join("\n"),
        });
        self
    }

    /// Append tone guidance: concise, direct, no tool narration.
    pub fn with_tone_and_style(mut self) -> Self {
        self.sections.push(Section {
            name: "tone_and_style",
            text: TONE_AND_STYLE_SECTION.into(),
        });
        self
    }

    /// Append output formatting guidance for terminal markdown rendering.
    pub fn with_output_format(mut self) -> Self {
        self.sections.push(Section {
            name: "output_format",
            text: OUTPUT_FORMAT_SECTION.into(),
        });
        self
    }

    /// Append the static/dynamic prompt boundary marker used by prompt-cache aware providers.
    pub fn with_dynamic_boundary(mut self) -> Self {
        self.sections.push(Section {
            name: "dynamic_boundary",
            text: DYNAMIC_BOUNDARY.into(),
        });
        self
    }

    /// Append output efficiency constraints: concise, no filler, lead with the answer.
    pub fn with_output_efficiency(mut self) -> Self {
        self.sections.push(Section {
            name: "output_efficiency",
            text: OUTPUT_EFFICIENCY_SECTION.into(),
        });
        self
    }

    /// Append clarifying-question guidance.
    pub fn with_clarifying_questions(mut self) -> Self {
        self.sections.push(Section {
            name: "clarifying_questions",
            text: CLARIFYING_QUESTIONS_SECTION.into(),
        });
        self
    }

    /// Append context management guidance for compacted tool results.
    pub fn with_context_management(mut self) -> Self {
        self.sections.push(Section {
            name: "context_management",
            text: CONTEXT_MANAGEMENT_SECTION.into(),
        });
        self
    }

    /// Append guidance on executing actions with care.
    pub fn with_executing_actions(mut self) -> Self {
        self.sections.push(Section {
            name: "executing_actions",
            text: EXECUTING_ACTIONS_SECTION.into(),
        });
        self
    }

    /// Append stable environment info: working dir, platform, shell, OS version.
    pub fn with_environment_static(mut self) -> Self {
        self.sections.push(Section {
            name: "environment",
            text: format!("Current working directory: {}", self.cwd),
        });
        self
    }

    /// Append dynamic date info.
    pub fn with_today_date(mut self) -> Self {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.sections.push(Section {
            name: "date",
            text: format!("Current date: {today}"),
        });
        self
    }

    /// Append environment info.
    pub fn with_environment(self) -> Self {
        self.with_environment_static().with_today_date()
    }

    /// Append the standard static guidance plus environment info (excluding date).
    /// Date should be added after the dynamic boundary to avoid busting prompt cache daily.
    pub fn with_system(self) -> Self {
        self.with_tool_guidance().with_environment_static()
    }

    /// Append git repository info: branch, default branch, user, status, recent commits.
    pub fn with_git(mut self) -> Self {
        let is_git = is_git_repo(&self.cwd);

        let mut lines = vec![format!(
            "Git repository: {}",
            if is_git { "yes" } else { "no" }
        )];

        if is_git {
            if let Some(git_info) = collect_git_info(&self.cwd) {
                lines.push(git_info);
            }
        }

        self.sections.push(Section {
            name: "git",
            text: format!("# Git\n\n{}", lines.join("\n")),
        });
        self
    }

    /// Load and append project context from well-known files.
    pub fn with_project_context(mut self) -> Self {
        let mut context = String::new();
        for name in PROJECT_CONTEXT_FILES {
            let path = Path::new(&self.cwd).join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let content = content.trim();
                if !content.is_empty() {
                    if !context.is_empty() {
                        context.push_str("\n\n");
                    }
                    context.push_str(content);
                }
            }
        }
        if !context.is_empty() {
            self.sections.push(Section {
                name: "project_instructions",
                text: format!("# Project Instructions\n\n{context}"),
            });
        }
        self
    }

    /// Append arbitrary text (e.g. user-supplied `--append-system-prompt`).
    pub fn with_append(mut self, text: &str) -> Self {
        self.sections.push(Section {
            name: "append",
            text: text.to_string(),
        });
        self
    }

    /// Consume the builder and produce the final prompt string.
    pub fn build(self) -> String {
        self.sections
            .into_iter()
            .map(|s| s.text)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Consume the builder and return both the joined prompt string and the
    /// per-section breakdown.
    pub fn build_with_sections(self) -> (String, Vec<Section>) {
        let sections: Vec<Section> = self
            .sections
            .into_iter()
            .filter(|s| !s.text.is_empty())
            .collect();
        let text = sections
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        (text, sections)
    }
}

// ---------------------------------------------------------------------------
// System helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

fn is_git_repo(cwd: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn collect_git_info(cwd: &str) -> Option<String> {
    let branch = run_git(cwd, &["branch", "--show-current"]).unwrap_or_default();
    let default_branch = detect_default_branch(cwd);
    let user = run_git(cwd, &["config", "user.name"]);
    let log = run_git(cwd, &["log", "--oneline", "-n", "5"]);

    let mut parts = Vec::new();

    if !branch.is_empty() {
        parts.push(format!("Current branch: {branch}"));
    }
    if let Some(main) = default_branch {
        parts.push(format!("Default branch: {main}"));
    }
    if let Some(u) = user {
        parts.push(format!("Git user: {u}"));
    }
    if let Some(l) = log {
        parts.push(format!("Recent commits:\n{l}"));
    }

    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

fn detect_default_branch(cwd: &str) -> Option<String> {
    if let Some(remote_head) = run_git(cwd, &["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        return remote_head
            .strip_prefix("refs/remotes/origin/")
            .map(String::from);
    }
    for candidate in &["main", "master"] {
        let exists = Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{candidate}"),
            ])
            .current_dir(cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if exists {
            return Some((*candidate).to_string());
        }
    }
    None
}

fn run_git(cwd: &str, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(["--no-optional-locks"])
        .args(args)
        .current_dir(cwd)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}
