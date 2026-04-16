//! Tool mode and tool-set construction.

use std::path::PathBuf;

use evot_engine::tools::*;

// ---------------------------------------------------------------------------
// ToolMode — determines which tools are registered for a query
// ---------------------------------------------------------------------------

pub enum ToolMode {
    /// REPL interactive: full tools + ask_user
    Interactive { ask_fn: AskUserFn },
    /// Oneshot / API / headless: full tools, no ask_user
    Headless,
    /// Plan mode: write tools degraded, optional ask_user
    Planning { ask_fn: Option<AskUserFn> },
    /// Forked conversation: read-only
    Readonly,
}

impl ToolMode {
    pub fn is_planning(&self) -> bool {
        matches!(self, Self::Planning { .. })
    }

    pub fn is_readonly(&self) -> bool {
        matches!(self, Self::Readonly)
    }
}

fn build_bash_tool(
    envs: Vec<(String, String)>,
    sandbox_dirs: Option<Vec<PathBuf>>,
) -> Box<dyn evot_engine::AgentTool> {
    let mut bash = BashTool::default().with_envs(envs);
    if let Some(dirs) = sandbox_dirs {
        bash = bash.with_sandbox_dirs(dirs);
    }
    Box::new(bash)
}

pub(crate) fn build_tools(
    mode: &ToolMode,
    envs: Vec<(String, String)>,
    allow_bash: bool,
    sandbox_dirs: Option<Vec<PathBuf>>,
) -> Vec<Box<dyn evot_engine::AgentTool>> {
    match mode {
        ToolMode::Interactive { ask_fn } => {
            let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();
            if allow_bash {
                t.push(build_bash_tool(envs, sandbox_dirs));
            }
            t.push(Box::new(ReadFileTool::default()));
            t.push(Box::new(WriteFileTool::new()));
            t.push(Box::new(EditFileTool::new()));
            t.push(Box::new(ListFilesTool::default()));
            t.push(Box::new(SearchTool::default()));
            t.push(Box::new(WebFetchTool::new()));
            t.push(Box::new(AskUserTool::new(ask_fn.clone())));
            t
        }
        ToolMode::Headless => {
            let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();
            if allow_bash {
                t.push(build_bash_tool(envs, sandbox_dirs));
            }
            t.push(Box::new(ReadFileTool::default()));
            t.push(Box::new(WriteFileTool::new()));
            t.push(Box::new(EditFileTool::new()));
            t.push(Box::new(ListFilesTool::default()));
            t.push(Box::new(SearchTool::default()));
            t.push(Box::new(WebFetchTool::new()));
            t
        }
        ToolMode::Planning { ask_fn } => {
            let msg = "Not allowed in planning mode. Use /act to switch.";
            let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();
            if allow_bash {
                t.push(build_bash_tool(envs, sandbox_dirs));
            }
            t.push(Box::new(ReadFileTool::default()));
            t.push(Box::new(WriteFileTool::new().disallow(msg)));
            t.push(Box::new(EditFileTool::new().disallow(msg)));
            t.push(Box::new(ListFilesTool::default()));
            t.push(Box::new(SearchTool::default()));
            t.push(Box::new(WebFetchTool::new()));
            if let Some(f) = ask_fn {
                t.push(Box::new(AskUserTool::new(f.clone())));
            }
            t
        }
        ToolMode::Readonly => vec![
            Box::new(ReadFileTool::default()),
            Box::new(ListFilesTool::default()),
            Box::new(SearchTool::default()),
        ],
    }
}
