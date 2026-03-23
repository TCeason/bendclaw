use super::RecallStore;
use crate::kernel::run::event::Event;
use crate::observability::log::slog;
use crate::storage::dal::knowledge::KnowledgeRecord;

/// Process tool events from a completed run and extract knowledge.
///
/// Only file_write/file_edit successes produce knowledge entries (structured metadata only).
/// Learnings are written exclusively by the agent via the `learning_write` tool.
/// Each event is processed independently — individual failures are logged and skipped.
pub async fn process_run_events(
    store: &RecallStore,
    run_id: &str,
    user_id: &str,
    events: &[Event],
) {
    // Collect arguments from ToolStart, keyed by tool_call_id
    let mut tool_args: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for event in events {
        if let Event::ToolStart {
            tool_call_id,
            arguments,
            ..
        } = event
        {
            tool_args.insert(tool_call_id.clone(), arguments.clone());
        }
    }

    for event in events {
        if let Event::ToolEnd {
            tool_call_id,
            name,
            success,
            ..
        } = event
        {
            if !success {
                continue;
            }
            let args = tool_args.get(tool_call_id);
            if let Err(e) = process_tool_success(store, run_id, user_id, name, args).await {
                slog!(warn, "recall", "skipped",
                    run_id,
                    tool = %name,
                    error = %e,
                );
            }
        }
    }
}

async fn process_tool_success(
    store: &RecallStore,
    run_id: &str,
    user_id: &str,
    tool_name: &str,
    args: Option<&serde_json::Value>,
) -> crate::base::Result<()> {
    match tool_name {
        "file_write" | "file_edit" => {
            let path = args
                .and_then(|a| a.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Ok(());
            }
            let record = KnowledgeRecord {
                id: crate::base::new_id(),
                kind: "file".to_string(),
                subject: tool_name.to_string(),
                locator: path.to_string(),
                title: format!("{tool_name}: {path}"),
                summary: format!("File modified by {tool_name}"),
                metadata: None,
                status: "active".to_string(),
                confidence: 1.0,
                user_id: user_id.to_string(),
                scope: "shared".to_string(),
                created_by: user_id.to_string(),
                first_run_id: run_id.to_string(),
                last_run_id: run_id.to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            };
            store.knowledge().insert(&record).await?;
        }
        _ => {}
    }
    Ok(())
}
