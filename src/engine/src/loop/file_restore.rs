//! Post-compaction file restore: re-inject recently read files whose content
//! was compacted away, so the model doesn't need to re-read them.

use std::path::Path;

use crate::context::estimate_tokens;
use crate::context::file_state::SharedFileReadState;
use crate::context::ContextConfig;
use crate::types::*;

const POST_COMPACT_MAX_FILES: usize = 5;
const POST_COMPACT_MAX_TOKENS_PER_FILE: usize = 5_000;
const POST_COMPACT_TOKEN_BUDGET: usize = 50_000;

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

    let paths_with_content = collect_paths_with_content(&messages[protected_start..]);

    let mut restore_parts: Vec<String> = Vec::new();
    let mut budget_used: usize = 0;

    for entry in &recent {
        if budget_used >= POST_COMPACT_TOKEN_BUDGET {
            break;
        }
        if paths_with_content.contains(&entry.path) {
            continue;
        }
        if !has_compacted_read(&messages[..protected_start], &entry.path) {
            continue;
        }

        let content = match tokio::fs::read_to_string(&entry.path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let truncated = truncate_to_token_budget(&content, POST_COMPACT_MAX_TOKENS_PER_FILE);
        let tokens = estimate_tokens(&truncated);
        budget_used += tokens;

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

fn extract_path_from_read_content(_content: &[Content]) -> Option<String> {
    None
}

fn truncate_to_token_budget(content: &str, max_tokens: usize) -> String {
    if estimate_tokens(content) <= max_tokens {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    let head_token_budget = max_tokens * 70 / 100;
    let tail_token_budget = max_tokens - head_token_budget;

    let mut head = String::new();
    for line in &lines {
        let candidate = if head.is_empty() {
            line.to_string()
        } else {
            format!("{}\n{}", head, line)
        };
        if estimate_tokens(&candidate) > head_token_budget {
            break;
        }
        head = candidate;
    }

    let mut tail = String::new();
    let mut tail_line_count: usize = 0;
    for line in lines.iter().rev() {
        let candidate = if tail.is_empty() {
            line.to_string()
        } else {
            format!("{}\n{}", line, tail)
        };
        if estimate_tokens(&candidate) > tail_token_budget {
            break;
        }
        tail = candidate;
        tail_line_count += 1;
    }

    let head_lines = head.lines().count();
    let omitted = total.saturating_sub(head_lines + tail_line_count);

    format!(
        "{}\n\n[... {} lines omitted ...]\n\n{}",
        head, omitted, tail
    )
}
