use std::sync::Arc;

use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::memory::MemoryService;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::ToolId;

/// Register optional tools that need extra services (cluster, memory).
/// Called conditionally based on service availability.
pub fn register(
    registry: &mut ToolRegistry,
    cluster: Option<(&Arc<ClusterService>, &Arc<DispatchTable>)>,
    memory: Option<&Arc<MemoryService>>,
) {
    if let Some((service, dispatch_table)) = cluster {
        registry.register_builtin(
            ToolId::ClusterNodes,
            Arc::new(
                crate::kernel::tools::builtins::cluster::ClusterNodesTool::new(service.clone()),
            ),
        );
        registry.register_builtin(
            ToolId::ClusterDispatch,
            Arc::new(
                crate::kernel::tools::builtins::cluster::ClusterDispatchTool::new(
                    service.clone(),
                    dispatch_table.clone(),
                ),
            ),
        );
        registry.register_builtin(
            ToolId::ClusterCollect,
            Arc::new(
                crate::kernel::tools::builtins::cluster::ClusterCollectTool::new(
                    dispatch_table.clone(),
                ),
            ),
        );
    }

    if let Some(mem) = memory {
        registry.register_builtin(
            ToolId::MemorySearch,
            Arc::new(
                crate::kernel::tools::builtins::memory::search::MemorySearchTool::new(mem.clone()),
            ),
        );
        registry.register_builtin(
            ToolId::MemorySave,
            Arc::new(
                crate::kernel::tools::builtins::memory::save::MemorySaveTool::new(mem.clone()),
            ),
        );
    }
}
