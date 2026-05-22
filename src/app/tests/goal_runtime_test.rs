//! Tests for goal runtime helpers.

use evot::agent::goal::runtime::turn_called_update_goal_tasks;
use evot::types::ToolCallRecord;
use evot::types::TranscriptItem;

#[test]
fn detects_update_goal_tasks_call() {
    let transcript = vec![TranscriptItem::Assistant {
        text: String::new(),
        thinking: None,
        tool_calls: vec![ToolCallRecord {
            id: "call-1".into(),
            name: "update_goal_tasks".into(),
            input: serde_json::json!({}),
        }],
        stop_reason: "tool_use".into(),
    }];

    assert!(turn_called_update_goal_tasks(&transcript));
}

#[test]
fn detects_update_goal_tasks_result() {
    let transcript = vec![TranscriptItem::ToolResult {
        tool_call_id: "call-1".into(),
        tool_name: "update_goal_tasks".into(),
        content: "updated".into(),
        is_error: false,
    }];

    assert!(turn_called_update_goal_tasks(&transcript));
}

#[test]
fn ignores_other_tool_calls_for_goal_task_plan() {
    let transcript = vec![TranscriptItem::ToolResult {
        tool_call_id: "call-1".into(),
        tool_name: "Bash".into(),
        content: "done".into(),
        is_error: false,
    }];

    assert!(!turn_called_update_goal_tasks(&transcript));
}
