#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolId {
    SkillRead,
    SkillCreate,
    SkillRemove,
    FileRead,
    FileWrite,
    FileEdit,
    ListDir,
    Shell,
    Databend,
    ChannelSend,
    WebSearch,
    WebFetch,
    TaskCreate,
    TaskList,
    TaskGet,
    TaskUpdate,
    TaskDelete,
    TaskToggle,
    TaskHistory,
    TaskRun,
    ClusterNodes,
    ClusterDispatch,
    ClusterCollect,
    Grep,
    Glob,
    MemorySearch,
    MemorySave,
}

impl ToolId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SkillRead => "skill_read",
            Self::SkillCreate => "create_skill",
            Self::SkillRemove => "remove_skill",
            Self::FileRead => "file_read",
            Self::FileWrite => "file_write",
            Self::FileEdit => "file_edit",
            Self::ListDir => "list_dir",
            Self::Shell => "shell",
            Self::Databend => "databend",
            Self::ChannelSend => "channel_send",
            Self::WebSearch => "web_search",
            Self::WebFetch => "web_fetch",
            Self::TaskCreate => "task_create",
            Self::TaskList => "task_list",
            Self::TaskGet => "task_get",
            Self::TaskUpdate => "task_update",
            Self::TaskDelete => "task_delete",
            Self::TaskToggle => "task_toggle",
            Self::TaskHistory => "task_history",
            Self::TaskRun => "task_run",
            Self::ClusterNodes => "cluster_nodes",
            Self::ClusterDispatch => "cluster_dispatch",
            Self::ClusterCollect => "cluster_collect",
            Self::Grep => "grep",
            Self::Glob => "glob",
            Self::MemorySearch => "memory_search",
            Self::MemorySave => "memory_save",
        }
    }
}

impl ToolId {
    /// Every `ToolId` variant, grouped by category.
    pub const ALL: &[ToolId] = &[
        // Skills
        ToolId::SkillRead,
        ToolId::SkillCreate,
        ToolId::SkillRemove,
        // Files
        ToolId::FileRead,
        ToolId::FileWrite,
        ToolId::FileEdit,
        ToolId::ListDir,
        ToolId::Grep,
        ToolId::Glob,
        // Shell
        ToolId::Shell,
        // Integrations
        ToolId::Databend,
        ToolId::ChannelSend,
        // Web
        ToolId::WebSearch,
        ToolId::WebFetch,
        // Tasks
        ToolId::TaskCreate,
        ToolId::TaskList,
        ToolId::TaskGet,
        ToolId::TaskUpdate,
        ToolId::TaskDelete,
        ToolId::TaskToggle,
        ToolId::TaskHistory,
        ToolId::TaskRun,
    ];

    /// Cluster tools, registered conditionally when cluster config is present.
    pub const CLUSTER: &[ToolId] = &[
        ToolId::ClusterNodes,
        ToolId::ClusterDispatch,
        ToolId::ClusterCollect,
    ];
}
