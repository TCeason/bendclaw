use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;

use reedline::Completer;
use reedline::Span;
use reedline::Suggestion;

use super::commands::command_arg_completions;
use super::commands::command_short_description;
use super::commands::KNOWN_COMMANDS;

// ---------------------------------------------------------------------------
// CompletionState — shared mutable state for dynamic completions
// ---------------------------------------------------------------------------

pub type CompletionStateRef = Arc<RwLock<CompletionState>>;

#[derive(Default)]
pub struct CompletionState {
    pub models: Vec<String>,
    pub session_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// ReplCompleter — implements reedline::Completer
// ---------------------------------------------------------------------------

pub struct ReplCompleter {
    state: CompletionStateRef,
}

impl ReplCompleter {
    pub fn new(state: CompletionStateRef) -> Self {
        Self { state }
    }
}

impl Completer for ReplCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let prefix = &line[..pos];

        // Slash-command completion (bare `/cmd`)
        if is_slash_prefix(prefix) && !prefix.contains(' ') {
            return KNOWN_COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|cmd| {
                    let desc = command_short_description(&cmd[1..]).unwrap_or("");
                    let display = if desc.is_empty() {
                        None
                    } else {
                        Some(format!("{cmd:<12} {desc}"))
                    };
                    Suggestion {
                        value: cmd.to_string(),
                        description: if desc.is_empty() {
                            None
                        } else {
                            Some(desc.to_string())
                        },
                        display_override: display,
                        span: Span::new(0, pos),
                        append_whitespace: true,
                        ..Default::default()
                    }
                })
                .collect();
        }

        // Slash-command argument completion (`/cmd arg`)
        if is_slash_prefix(prefix) {
            if let Some(space_pos) = prefix.find(' ') {
                let cmd = &prefix[..space_pos];
                let arg_part = &prefix[space_pos + 1..];
                if !arg_part.contains(' ') {
                    if let Ok(state) = self.state.read() {
                        let candidates = command_arg_completions(cmd, arg_part, &state);
                        if !candidates.is_empty() {
                            return candidates
                                .into_iter()
                                .map(|candidate| Suggestion {
                                    value: candidate.clone(),
                                    span: Span::new(space_pos + 1, pos),
                                    append_whitespace: false,
                                    ..Default::default()
                                })
                                .collect();
                        }
                    }
                }
            }
        }

        // File path completion
        let word_start = prefix.rfind(char::is_whitespace).map_or(0, |i| i + 1);
        let word = &prefix[word_start..];
        if word.is_empty() {
            return Vec::new();
        }

        complete_file_path(word)
            .into_iter()
            .map(|value| Suggestion {
                value,
                span: Span::new(word_start, pos),
                append_whitespace: false,
                ..Default::default()
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (re-exported for tests)
// ---------------------------------------------------------------------------

/// Returns `true` when `text` looks like a hand-typed slash command prefix:
/// `/` followed by zero or more ASCII lowercase letters (and optionally `!`).
/// Pasted paths like `/some/path.rs` or `:/foo/bar` are rejected because
/// they contain characters that never appear in a valid command name.
pub fn is_slash_prefix(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('/') else {
        return false;
    };
    // After the leading `/`, allow only ASCII letters and `!` (for `/clear!`).
    // A space is fine — it separates the command from its argument — but
    // anything before the first space must be pure command chars.
    // A bare `/` (rest is empty) is also valid — it triggers the full command list.
    let cmd_part = rest.split_once(' ').map_or(rest, |(cmd, _)| cmd);
    cmd_part
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b == b'!')
}

/// Build the display string shown as a hint when the user types a bare `/`.
pub fn bare_slash_hint_display() -> String {
    let names: String = KNOWN_COMMANDS
        .iter()
        .map(|cmd| &cmd[1..])
        .collect::<Vec<_>>()
        .join("  ");
    format!("  [{names}]")
}

pub fn complete_file_path(partial: &str) -> Vec<String> {
    let path = Path::new(partial);

    let (dir, file_prefix) =
        if partial.ends_with('/') || partial.ends_with(std::path::MAIN_SEPARATOR) {
            (partial.to_string(), String::new())
        } else if let Some(parent) = path.parent() {
            let parent_str = if parent.as_os_str().is_empty() {
                ".".to_string()
            } else {
                parent.to_string_lossy().to_string()
            };
            let file_prefix = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            (parent_str, file_prefix)
        } else {
            (".".to_string(), partial.to_string())
        };

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let dir_prefix = if dir == "." && !partial.contains('/') {
        String::new()
    } else if partial.ends_with('/') || partial.ends_with(std::path::MAIN_SEPARATOR) {
        partial.to_string()
    } else {
        let parent = path.parent().unwrap_or(Path::new(""));
        if parent.as_os_str().is_empty() {
            String::new()
        } else {
            format!("{}/", parent.display())
        }
    };

    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&file_prefix) {
            continue;
        }
        let is_dir = entry
            .file_type()
            .map(|value| value.is_dir())
            .unwrap_or(false);
        let candidate = if is_dir {
            format!("{}{}/", dir_prefix, name)
        } else {
            format!("{}{}", dir_prefix, name)
        };
        matches.push(candidate);
    }
    matches.sort();
    matches
}
