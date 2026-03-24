use bendclaw::kernel::run::tool_outcome_guard::ToolDispatchReport;
use bendclaw::kernel::run::tool_outcome_guard::ToolOutcomeGuard;

#[test]
fn records_grounding_message_with_execution_summary() {
    let mut guard = ToolOutcomeGuard::default();
    guard.record(ToolDispatchReport {
        requested: vec!["task_run".into(), "channel_send".into()],
        succeeded: vec!["task_run".into()],
        failed: vec!["channel_send".into()],
        blocked: Vec::new(),
        skipped: Vec::new(),
    });

    let grounding = guard.take_grounding().expect("grounding message");
    assert!(grounding.contains("Requested tools: task_run, channel_send."));
    assert!(grounding.contains("Successful tools: task_run."));
    assert!(grounding.contains("Failed tools: channel_send."));
    assert!(grounding.contains("Do not claim an action was completed"));
}

#[test]
fn take_grounding_consumes_pending_message() {
    let mut guard = ToolOutcomeGuard::default();
    guard.record(ToolDispatchReport {
        requested: vec!["task_list".into()],
        succeeded: vec!["task_list".into()],
        failed: Vec::new(),
        blocked: Vec::new(),
        skipped: vec!["task_run".into()],
    });

    assert!(guard.take_grounding().is_some());
    assert!(guard.take_grounding().is_none());
}
