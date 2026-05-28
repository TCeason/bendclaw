use evot_engine::tools::*;

#[derive(Clone)]
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
