use std::path::Path;

const PROJECT_CONTEXT_FILES: &[&str] = &["EVOT.md", "CLAUDE.md", "AGENTS.md"];

/// Marks where the cacheable static prefix ends and per-turn content begins.
/// Prompt-cache aware providers split the system prompt here.
const DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

const USING_TOOLS_HEADER: &str = "Using your tools:";
// When bash is present but no dedicated exploration tools are registered (the
// pi-aligned default coding set: read, bash, edit, write), point the model at
// bash for search/listing instead of discouraging it. Matches pi's guidance.
const BASH_EXPLORE_GUIDELINE: &str = "Use bash for file operations like ls, rg, find";
// When dedicated exploration tools ARE present (e.g. read-only mode ships grep
// and glob), frame the dedicated-vs-bash tradeoff the other way.
const USING_TOOLS_INTRO: &str = "Do not run a bash command when a dedicated tool exists for the task — dedicated tools are easier for the user to review and give you cleaner, structured results.";
const BASH_FILE_OPS_GUIDELINE: &str = "Reserve bash for system commands and terminal operations that need a shell. When unsure and a dedicated tool exists, default to it and fall back to bash only when necessary.";
const USING_TOOLS_TRAILER: &[&str] = &["Show file paths clearly when working with files"];

const IDENTITY_INTRO: &str =
    "You are an expert coding assistant. You help users by reading files, \
     executing commands, editing code, and writing new files.";
const IDENTITY_OUTRO: &str =
    "In addition to the tools above, you may have access to other custom tools depending on the project.";

const OUTPUT_FORMAT_SECTION: &str = "\
Your responses render as GitHub-flavored markdown in a terminal. Use code \
blocks for code and commands, and reference locations as `path:line`. Avoid \
LaTeX math delimiters ($, $$) the terminal cannot render — use plain text or \
Unicode.";

const OUTPUT_EFFICIENCY_SECTION: &str = "\
Keep responses concise, direct, and proportional to the task. Be concise in \
prose, not in evidence. Skip filler and meta-commentary.\n\
- For simple questions, confirmations, or small changes, give the outcome in a \
short answer without headings or heavy formatting.\n\
- Lead with the outcome, not a chronological account of your work. For \
substantial work, explain only the material changes and rationale.\n\
- Include technical detail only when it helps the user understand, verify, or \
act on the result. Calibrate detail to the user's apparent expertise.\n\
- Use the minimum formatting needed. Merge related points and keep lists short.\n\
- Summarize successful commands and tests by their result. Include exact output \
only when the user asks for it or when failure details are needed.\n\
- Do not repeat the request, plans, patches, large files, or tool output already \
visible to the user; reference the relevant location instead.\n\
- Suggest next steps only when they are natural and useful. Do not add a generic \
offer to do more.\n\
- Report blockers and failures plainly. State skipped verification, and state \
completed, verified work without hedging.";

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
    /// Whether the tool set includes dedicated exploration tools (grep/glob/
    /// semantic search). When false (the pi-aligned default coding set), the
    /// prompt steers search through bash instead of discouraging it.
    has_dedicated_search: bool,
    /// Pre-rendered "prefer this dedicated tool" lines, one per tool that
    /// declares `prefer_over`, with the tool name already resolved to the
    /// alias the target model sees.
    prefer_lines: Vec<String>,
    tools_guidelines: Vec<String>,
    sections: Vec<Section>,
}

impl SystemPrompt {
    /// Build the static system-prompt base used for every turn.
    ///
    /// This is the process-constant prefix: tool-aware identity, output rules,
    /// project context, then the cache boundary and date. Per-turn content
    /// (mode, sandbox, variables) is appended separately by
    /// [`super::dynamic_sections`] and lands after the boundary.
    pub fn base(
        cwd: &str,
        tools: &[Box<dyn evot_engine::AgentTool>],
        model: &str,
    ) -> (String, Vec<Section>) {
        Self::with_tool_set_for_model(cwd, tools, model)
            .with_tool_guidance()
            .with_environment_static()
            .with_output_format()
            .with_output_efficiency()
            .with_project_context()
            .with_dynamic_boundary()
            .with_today_date()
            .build_with_sections()
    }

    /// Construct a prompt whose identity "Available tools" list is derived from
    /// the given tools: a tool appears only when it provides a snippet, and its
    /// name is rendered using the alias the target `model` actually sees (e.g.
    /// Claude is offered `Grep`/`Glob`/`Read`, so the prompt must say the same
    /// — otherwise the advertised name won't match the tool the model can call).
    /// The guidelines section is assembled in tool order with dedup.
    pub fn with_tool_set_for_model(
        cwd: &str,
        tools: &[Box<dyn evot_engine::AgentTool>],
        model: &str,
    ) -> Self {
        let mut text = String::from(IDENTITY_INTRO);
        let listed: Vec<&Box<dyn evot_engine::AgentTool>> = tools
            .iter()
            .filter(|t| t.prompt_snippet().is_some())
            .collect();
        if !listed.is_empty() {
            text.push_str("\n\nAvailable tools:");
            for t in &listed {
                if let Some(snippet) = t.prompt_snippet() {
                    text.push_str(&format!("\n- {}: {}", t.resolve_name(model), snippet));
                }
            }
            text.push_str("\n\n");
            text.push_str(IDENTITY_OUTRO);
        }
        Self {
            cwd: cwd.to_string(),
            has_bash: tools.iter().any(|t| t.name() == "bash"),
            has_dedicated_search: tools
                .iter()
                .any(|t| matches!(t.name(), "grep" | "glob" | "semantic_code_search")),
            prefer_lines: tools
                .iter()
                .filter_map(|t| {
                    t.prefer_over().map(|(action, alternatives)| {
                        format!(
                            "To {action}, use `{}` instead of {alternatives}.",
                            t.resolve_name(model)
                        )
                    })
                })
                .collect(),
            tools_guidelines: tools
                .iter()
                .flat_map(|t| {
                    t.prompt_guidelines()
                        .into_iter()
                        .map(|g| evot_engine::tools::resolve_tool_refs(g, tools, model))
                        .collect::<Vec<_>>()
                })
                .collect(),
            sections: vec![Section {
                name: "identity",
                text,
            }],
        }
    }

    /// Append tool-use guidance in the style of a short "Using your tools"
    /// section: a framing principle, the per-tool "prefer this dedicated tool"
    /// lines (rendered with model-resolved alias names), the bash fallback
    /// rule, then per-tool mechanics and the shared trailer — all deduplicated.
    pub fn with_tool_guidance(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        let mut lines: Vec<String> = vec![USING_TOOLS_HEADER.to_string()];
        let mut add = |s: &str| {
            let s = s.trim();
            if !s.is_empty() && seen.insert(s.to_string()) {
                lines.push(format!("- {s}"));
            }
        };

        // File-exploration framing depends on whether dedicated search tools
        // are registered, mirroring pi's system-prompt logic:
        //   - no dedicated tools (default coding set): steer search to bash.
        //   - dedicated tools present (read-only mode): prefer them over bash.
        if self.has_bash && !self.has_dedicated_search {
            add(BASH_EXPLORE_GUIDELINE);
        } else if self.has_bash {
            add(USING_TOOLS_INTRO);
        }
        for line in &self.prefer_lines {
            add(line);
        }
        if self.has_bash && self.has_dedicated_search {
            add(BASH_FILE_OPS_GUIDELINE);
        }
        for g in &self.tools_guidelines {
            add(g);
        }
        for t in USING_TOOLS_TRAILER {
            add(t);
        }

        self.sections.push(Section {
            name: "using_tools",
            text: lines.join("\n"),
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

    /// Append output efficiency constraints: concise, no filler, lead with the answer.
    pub fn with_output_efficiency(mut self) -> Self {
        self.sections.push(Section {
            name: "output_efficiency",
            text: OUTPUT_EFFICIENCY_SECTION.into(),
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

    /// Append the stable working-directory line.
    pub fn with_environment_static(mut self) -> Self {
        self.sections.push(Section {
            name: "environment",
            text: format!("Current working directory: {}", self.cwd),
        });
        self
    }

    /// Append the current date. Added after the dynamic boundary so the daily
    /// change does not bust the prompt cache.
    pub fn with_today_date(mut self) -> Self {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.sections.push(Section {
            name: "date",
            text: format!("Current date: {today}"),
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

    /// Consume the builder and produce the final prompt string.
    pub fn build(self) -> String {
        self.build_with_sections().0
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
