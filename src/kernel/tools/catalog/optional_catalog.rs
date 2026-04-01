use std::sync::Arc;

use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::memory::MemoryService;
use crate::kernel::tools::catalog::tool_registry::ToolRegistry;
use crate::kernel::tools::ToolId;

pub(crate) fn register_optional(
    registry: &mut ToolRegistry,
    cluster: Option<&(Arc<ClusterService>, Arc<DispatchTable>)>,
    memory: Option<&Arc<MemoryService>>,
) {
    if let Some((service, dispatch_table)) = cluster {
        use crate::kernel::tools::builtin::cluster::collect::ClusterCollectTool;
        use crate::kernel::tools::builtin::cluster::dispatch::ClusterDispatchTool;
        use crate::kernel::tools::builtin::cluster::nodes::ClusterNodesTool;

        registry.register_builtin(
            ToolId::ClusterNodes,
            Arc::new(ClusterNodesTool::new(service.clone())),
        );
        registry.register_builtin(
            ToolId::ClusterDispatch,
            Arc::new(ClusterDispatchTool::new(
                service.clone(),
                dispatch_table.clone(),
            )),
        );
        registry.register_builtin(
            ToolId::ClusterCollect,
            Arc::new(ClusterCollectTool::new(dispatch_table.clone())),
        );
    }

    if let Some(mem) = memory {
        use crate::kernel::tools::builtin::memory::save::MemorySaveTool;
        use crate::kernel::tools::builtin::memory::search::MemorySearchTool;

        registry.register_builtin(
            ToolId::MemorySearch,
            Arc::new(MemorySearchTool::new(mem.clone())),
        );
        registry.register_builtin(
            ToolId::MemorySave,
            Arc::new(MemorySaveTool::new(mem.clone())),
        );
    }
}
