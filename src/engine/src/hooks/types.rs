use std::sync::Arc;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    PostSampling,
    SessionStart,
    SessionEnd,
    Stop,
    SubagentStart,
    SubagentStop,
    UserPromptSubmit,
    PermissionRequest,
    PermissionDenied,
    TaskCreated,
    TaskCompleted,
    ConfigChange,
    CwdChanged,
    FileChanged,
    Notification,
    PreCompact,
    PostCompact,
    TeammateIdle,
}

#[derive(Debug, Clone)]
pub struct HookInput {
    pub event: HookEvent,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_output: Option<String>,
    pub tool_use_id: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct HookOutput {
    pub blocked: bool,
    pub message: Option<String>,
    pub permission_update: Option<PermissionUpdate>,
    pub notification: Option<HookNotification>,
}

#[derive(Debug, Clone)]
pub struct PermissionUpdate {
    pub tool: String,
    pub behavior: PermissionBehavior,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionBehavior {
    Allow,
    Deny,
}

#[derive(Debug, Clone)]
pub struct HookNotification {
    pub title: String,
    pub body: String,
    pub level: Option<NotificationLevel>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
}

pub type HookFn =
    Arc<dyn Fn(HookInput) -> futures::future::BoxFuture<'static, HookOutput> + Send + Sync>;

pub struct HookRule {
    pub matcher: String,
    pub handler: HookFn,
}
