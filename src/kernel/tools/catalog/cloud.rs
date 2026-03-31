use std::sync::Arc;

use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::runtime::org::OrgServices;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::ToolId;
use crate::storage::Pool;

/// Register tools that require Pool/OrgServices. Only for persistent sessions.
pub fn register(
    registry: &mut ToolRegistry,
    org: Arc<OrgServices>,
    databend_pool: Pool,
    channels: Arc<ChannelRegistry>,
    node_id: String,
) {
    // Skill tools
    registry.register_builtin(
        ToolId::SkillRead,
        Arc::new(crate::kernel::tools::builtins::skill::SkillReadTool::new(
            org.skills().clone(),
        )),
    );
    registry.register_builtin(
        ToolId::SkillCreate,
        Arc::new(crate::kernel::tools::builtins::skill::SkillCreateTool::new(
            org.skills().clone(),
        )),
    );
    registry.register_builtin(
        ToolId::SkillRemove,
        Arc::new(crate::kernel::tools::builtins::skill::SkillRemoveTool::new(
            org.skills().clone(),
        )),
    );

    // Databend
    registry.register_builtin(
        ToolId::Databend,
        Arc::new(crate::kernel::tools::builtins::databend::DatabendTool::new(
            databend_pool,
        )),
    );

    // Channel send
    registry.register_builtin(
        ToolId::ChannelSend,
        Arc::new(crate::kernel::tools::builtins::channel::ChannelSendTool::new(channels)),
    );

    // Task tools
    registry.register_builtin(
        ToolId::TaskCreate,
        Arc::new(crate::kernel::tools::builtins::task::TaskCreateTool::new(
            node_id.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::TaskList,
        Arc::new(crate::kernel::tools::builtins::task::TaskListTool::new(
            node_id.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::TaskGet,
        Arc::new(crate::kernel::tools::builtins::task::TaskGetTool::new(
            node_id.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::TaskUpdate,
        Arc::new(crate::kernel::tools::builtins::task::TaskUpdateTool::new(
            node_id.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::TaskDelete,
        Arc::new(crate::kernel::tools::builtins::task::TaskDeleteTool::new(
            node_id.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::TaskToggle,
        Arc::new(crate::kernel::tools::builtins::task::TaskToggleTool::new(
            node_id.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::TaskHistory,
        Arc::new(crate::kernel::tools::builtins::task::TaskHistoryTool::new(
            node_id,
        )),
    );
    registry.register_builtin(
        ToolId::TaskRun,
        Arc::new(crate::kernel::tools::builtins::task::TaskRunTool),
    );
}
