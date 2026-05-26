//! Post-compaction file restore: re-inject recently read files whose content
//! was compacted away, so the model doesn't need to re-read them.

use std::path::Path;

use crate::context::file_state::SharedFileReadState;
use crate::context::ContextConfig;
use crate::types::*;

const POST_COMPACT_MAX_FILES: usize = 3;
const POST_COMPACT_MAX_BYTES_PER_FILE: usize = 8_000;
const POST_COMPACT_TOTAL_BUDGET_BYTES: usize = 20_000;

/// After request-view compaction, check if recently read files lost their content
/// and re-inject a condensed version so the model retains working context.
pub(super) async fn maybe_restore_compacted_files(
    mut messages: Vec<Message>,
    ctx_config: Option<&ContextConfig>,
    file_read_state: &Option<SharedFileReadState>,
) -> Vec<Message> {
    let Some(state) = file_read_state else {
        return messages;
    };
    let guard = state.lock().await;
    let recent = guard.recent_files(POST_COMPACT_MAX_FILES);
    if recent.is_empty() {
        return messages;
    }

    let keep_recent = ctx_config.map(|c| c.keep_recent).unwrap_or(10);
    let protected_start = messages.len().saturating_sub(keep_recent);

    // Collect paths that still have substantial Read content in the protected tail
    let paths_with_content = collect_paths_with_content(&messages[protected_start..]);

    // Build restore content for files that lost their content
    let mut restore_parts: Vec<String> = Vec::new();
    let mut budget_used: usize = 0;

    for entry in &recent {
        if budget_used >= POST_COMPACT_TOTAL_BUDGET_BYTES {
            break;
        }
        if paths_with_content.contains(&entry.path) {
            continue;
        }
        // Check if this file was actually compacted (has a cleared/omitted marker in old messages)
        if !has_compacted_read(&messages[..protected_start], &entry.path) {
            continue;
        }

        let content = match tokio::fs::read_to_string(&entry.path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let truncated = truncate_head_tail(&content, POST_COMPACT_MAX_BYTES_PER_FILE);
        budget_used += truncated.len();

        restore_parts.push(format!(
            "--- {} ({} lines) ---\n{}",
            entry.path, entry.total_lines, truncated
        ));
    }

    drop(guard);

    if restore_parts.is_empty() {
        return messages;
    }

    let restore_text = format!(
        "[Context restored — these files were read earlier but compacted for space:]\n\n{}",
        restore_parts.join("\n\n")
    );

    let restore_msg = Message::User {
        content: vec![Content::Text { text: restore_text }],
        timestamp: now_ms(),
    };

    messages.insert(protected_start, restore_msg);
    messages
}

/// Collect file paths that have substantial Read content in the given messages.
fn collect_paths_with_content(messages: &[Message]) -> std::collections::HashSet<String> {
    let mut paths = std::collections::HashSet::new();
    for msg in messages {
        if let Message::ToolResult {
            tool_name, content, ..
        } = msg
        {
            if tool_name == "Read" || tool_name == "ReadSlim" {
                let text_len: usize = content
                    .iter()
                    .map(|c| match c {
                        Content::Text { text } => text.len(),
                        _ => 0,
                    })
                    .sum();
                // Only count as "has content" if it's more than a stub/marker
                if text_len > 200 {
                    if let Some(path) = extract_path_from_read_content(content) {
                        paths.insert(path);
                    }
                }
            }
        }
    }
    paths
}

/// Check if any compacted Read result in the old messages references this path.
fn has_compacted_read(messages: &[Message], path: &str) -> bool {
    let path_component = Path::new(path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(path);

    for msg in messages {
        if let Message::ToolResult {
            tool_name, content, ..
        } = msg
        {
            if tool_name != "Read" && tool_name != "ReadSlim" {
                continue;
            }
            for c in content {
                if let Content::Text { text } = c {
                    // Markers from microcompact: "[cleared — path, N lines]"
                    // Markers from request-view: "[tool result omitted..."
                    let is_marker = text.contains("[cleared")
                        || text.contains("[tool result omitted")
                        || text.contains("[omitted");
                    if is_marker && text.contains(path_component) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Extract file path from Read tool result content (first line header).
fn extract_path_from_read_content(_content: &[Content]) -> Option<String> {
    // Read results don't embed the path in content directly.
    // We can't reliably extract it here without the tool_call args.
    // For now, we skip this — the dedup mechanism is the primary defense.
    None
}

/// Truncate file content to head + tail within a byte budget.
fn truncate_head_tail(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    // Allocate 70% to head, 30% to tail
    let head_budget = max_bytes * 70 / 100;
    let tail_budget = max_bytes - head_budget;

    let mut head = String::new();
    for line in &lines {
        if head.len() + line.len() + 1 > head_budget {
            break;
        }
        if !head.is_empty() {
            head.push('\n');
        }
        head.push_str(line);
    }

    let mut tail_lines: Vec<&str> = Vec::new();
    let mut tail_size = 0;
    for line in lines.iter().rev() {
        if tail_size + line.len() + 1 > tail_budget {
            break;
        }
        tail_size += line.len() + 1;
        tail_lines.push(line);
    }
    tail_lines.reverse();
    let tail = tail_lines.join("\n");

    format!(
        "{}\n\n[... {} lines omitted ...]\n\n{}",
        head,
        total.saturating_sub(head.lines().count() + tail_lines.len()),
        tail
    )
}
