//! Tool-set construction for different agent modes.

use std::path::PathBuf;

use evot_engine::tools::*;

use super::ToolMode;

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
    if matches!(mode, ToolMode::Readonly) {
        return vec![Box::new(ReadFileTool::default())];
    }

    let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();

    // Core tool order mirrors the pi harness: read, bash, edit, write.
    t.push(Box::new(ReadFileTool::default()));

    if allow_bash {
        t.push(build_bash_tool(envs, sandbox_dirs));
    }

    if let ToolMode::Planning { .. } = mode {
        let msg = "Not allowed in planning mode. Use /act to switch.";
        t.push(Box::new(EditFileTool::new().disallow(msg)));
        t.push(Box::new(WriteFileTool::new().disallow(msg)));
    } else {
        t.push(Box::new(EditFileTool::new()));
        t.push(Box::new(WriteFileTool::new()));
    }

    // evot-specific tools, appended after the pi-aligned core set.
    if !matches!(mode, ToolMode::Headless) {
        t.push(Box::new(WebFetchTool::new()));
    }

    match mode {
        ToolMode::Interactive { ask_fn } => {
            t.push(Box::new(AskUserTool::new(ask_fn.clone())));
        }
        ToolMode::Planning { ask_fn: Some(f) } => {
            t.push(Box::new(AskUserTool::new(f.clone())));
        }
        _ => {}
    }

    t
}
