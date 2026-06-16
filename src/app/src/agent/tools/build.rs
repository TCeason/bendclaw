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
        // Read-only mode: read + structured search, no mutation or shell.
        return vec![
            Box::new(ReadFileTool::default()),
            Box::new(GrepTool::new()),
            Box::new(GlobTool::new()),
            Box::new(SearchTool::new()),
        ];
    }

    let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();

    // Core tool set: read, bash, edit, write, plus the dedicated explore tools.
    // grep and glob are builtin everywhere; semantic_code_search is added in all
    // modes except Headless. The explore tools run in-process on ripgrep/fd's
    // own engines (parallel, gitignore-aware) and give the model line-numbered,
    // structured output it can act on without re-reading files — strictly better
    // than shelling out to bash grep/find.
    t.push(Box::new(ReadFileTool::default()));

    // grep and glob are builtin in every mode: they run in-process on
    // ripgrep/fd's own engines (parallel, gitignore-aware) and add no startup
    // cost, so even short-lived Headless requests benefit from line-numbered,
    // structured search/find instead of shelling out to bash rg/find.
    t.push(Box::new(GrepTool::new()));
    t.push(Box::new(GlobTool::new()));

    // Semantic code search is gated out of Headless: it builds a full index of
    // the (possibly unknown, large) repo on first use, which isn't worth it for
    // oneshot/API requests.
    if !matches!(mode, ToolMode::Headless) {
        t.push(Box::new(SearchTool::new()));
    }

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

/// Canonical tool set used to assemble the system prompt's tool list and
/// guidelines at startup for the gateway, which runs in Headless mode. Must
/// stay in sync with the Headless set built by `build_tools`: read, grep, glob,
/// bash, edit, write (no semantic_code_search, no webfetch, no ask).
pub(crate) fn prompt_tools() -> Vec<Box<dyn evot_engine::AgentTool>> {
    vec![
        Box::new(ReadFileTool::default()),
        Box::new(GrepTool::new()),
        Box::new(GlobTool::new()),
        Box::new(BashTool::default()),
        Box::new(EditFileTool::new()),
        Box::new(WriteFileTool::new()),
    ]
}
