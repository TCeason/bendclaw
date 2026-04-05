use serde_json::Value;

use crate::hooks::HookEvent;
use crate::hooks::HookInput;
use crate::hooks::HookOutput;
use crate::hooks::HookRule;

#[derive(Default)]
pub struct HookConfig {
    pub pre_tool_use: Vec<HookRule>,
    pub post_tool_use: Vec<HookRule>,
    pub post_tool_use_failure: Vec<HookRule>,
    pub post_sampling: Vec<HookRule>,
    pub session_start: Vec<HookRule>,
    pub session_end: Vec<HookRule>,
    pub stop: Vec<HookRule>,
    pub subagent_start: Vec<HookRule>,
    pub subagent_stop: Vec<HookRule>,
    pub user_prompt_submit: Vec<HookRule>,
    pub permission_request: Vec<HookRule>,
    pub permission_denied: Vec<HookRule>,
    pub task_created: Vec<HookRule>,
    pub task_completed: Vec<HookRule>,
    pub config_change: Vec<HookRule>,
    pub cwd_changed: Vec<HookRule>,
    pub file_changed: Vec<HookRule>,
    pub notification: Vec<HookRule>,
    pub pre_compact: Vec<HookRule>,
    pub post_compact: Vec<HookRule>,
    pub teammate_idle: Vec<HookRule>,
}

impl HookConfig {
    pub fn rules_for_event(&self, event: &HookEvent) -> &[HookRule] {
        match event {
            HookEvent::PreToolUse => &self.pre_tool_use,
            HookEvent::PostToolUse => &self.post_tool_use,
            HookEvent::PostToolUseFailure => &self.post_tool_use_failure,
            HookEvent::PostSampling => &self.post_sampling,
            HookEvent::SessionStart => &self.session_start,
            HookEvent::SessionEnd => &self.session_end,
            HookEvent::Stop => &self.stop,
            HookEvent::SubagentStart => &self.subagent_start,
            HookEvent::SubagentStop => &self.subagent_stop,
            HookEvent::UserPromptSubmit => &self.user_prompt_submit,
            HookEvent::PermissionRequest => &self.permission_request,
            HookEvent::PermissionDenied => &self.permission_denied,
            HookEvent::TaskCreated => &self.task_created,
            HookEvent::TaskCompleted => &self.task_completed,
            HookEvent::ConfigChange => &self.config_change,
            HookEvent::CwdChanged => &self.cwd_changed,
            HookEvent::FileChanged => &self.file_changed,
            HookEvent::Notification => &self.notification,
            HookEvent::PreCompact => &self.pre_compact,
            HookEvent::PostCompact => &self.post_compact,
            HookEvent::TeammateIdle => &self.teammate_idle,
        }
    }

    pub async fn run_event(
        &self,
        event: HookEvent,
        tool_name: Option<&str>,
        tool_input: Option<&Value>,
        tool_output: Option<&str>,
    ) -> Vec<HookOutput> {
        let rules = self.rules_for_event(&event);
        let mut outputs = Vec::new();

        for rule in rules {
            if let Some(name) = tool_name {
                if !matches_tool(&rule.matcher, name) {
                    continue;
                }
            }

            let input = HookInput {
                event: event.clone(),
                tool_name: tool_name.map(|s| s.to_string()),
                tool_input: tool_input.cloned(),
                tool_output: tool_output.map(|s| s.to_string()),
                tool_use_id: None,
                session_id: None,
                cwd: None,
                error: None,
            };

            let output = (rule.handler)(input).await;
            outputs.push(output);
        }

        outputs
    }

    pub async fn run_pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: &Value,
    ) -> Option<HookOutput> {
        for rule in &self.pre_tool_use {
            if matches_tool(&rule.matcher, tool_name) {
                let input = HookInput {
                    event: HookEvent::PreToolUse,
                    tool_name: Some(tool_name.to_string()),
                    tool_input: Some(tool_input.clone()),
                    tool_output: None,
                    tool_use_id: None,
                    session_id: None,
                    cwd: None,
                    error: None,
                };
                let output = (rule.handler)(input).await;
                if output.blocked {
                    return Some(output);
                }
            }
        }

        None
    }

    pub async fn run_post_tool_use(&self, tool_name: &str, tool_input: &Value, tool_output: &str) {
        for rule in &self.post_tool_use {
            if matches_tool(&rule.matcher, tool_name) {
                let input = HookInput {
                    event: HookEvent::PostToolUse,
                    tool_name: Some(tool_name.to_string()),
                    tool_input: Some(tool_input.clone()),
                    tool_output: Some(tool_output.to_string()),
                    tool_use_id: None,
                    session_id: None,
                    cwd: None,
                    error: None,
                };
                (rule.handler)(input).await;
            }
        }
    }

    pub async fn run_stop(&self) {
        for rule in &self.stop {
            let input = HookInput {
                event: HookEvent::Stop,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                tool_use_id: None,
                session_id: None,
                cwd: None,
                error: None,
            };
            (rule.handler)(input).await;
        }
    }
}

fn matches_tool(matcher: &str, tool_name: &str) -> bool {
    if matcher == "*" || matcher.is_empty() {
        return true;
    }

    if matcher.contains('|') {
        return matcher
            .split('|')
            .any(|pattern| matches_tool(pattern.trim(), tool_name));
    }

    if let Some(prefix) = matcher.strip_suffix('*') {
        return tool_name.starts_with(prefix);
    }

    matcher == tool_name
}
