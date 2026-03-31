use std::sync::Arc;

use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::services::SecretUsageSink;
use crate::kernel::tools::ToolId;

/// Register core tools: file, search, shell, web.
/// Zero platform dependencies — only need a SecretUsageSink for shell/web secret touch.
pub fn register(registry: &mut ToolRegistry, secret_sink: Arc<dyn SecretUsageSink>) {
    // File tools
    registry.register_builtin(
        ToolId::FileRead,
        Arc::new(crate::kernel::tools::builtins::file::FileReadTool),
    );
    registry.register_builtin(
        ToolId::FileWrite,
        Arc::new(crate::kernel::tools::builtins::file::FileWriteTool),
    );
    registry.register_builtin(
        ToolId::FileEdit,
        Arc::new(crate::kernel::tools::builtins::file::FileEditTool),
    );
    registry.register_builtin(
        ToolId::ListDir,
        Arc::new(crate::kernel::tools::builtins::file::ListDirTool),
    );

    // Search tools
    registry.register_builtin(
        ToolId::Grep,
        Arc::new(crate::kernel::tools::builtins::search::GrepTool),
    );
    registry.register_builtin(
        ToolId::Glob,
        Arc::new(crate::kernel::tools::builtins::search::GlobTool),
    );

    // Shell
    registry.register_builtin(
        ToolId::Shell,
        Arc::new(crate::kernel::tools::builtins::shell::ShellTool::new(
            secret_sink.clone(),
        )),
    );

    // Web
    registry.register_builtin(
        ToolId::WebSearch,
        Arc::new(crate::kernel::tools::builtins::web::WebSearchTool::new(
            "https://api.search.brave.com/res/v1/web/search",
            secret_sink,
        )),
    );
    registry.register_builtin(
        ToolId::WebFetch,
        Arc::new(crate::kernel::tools::builtins::web::WebFetchTool),
    );
}
