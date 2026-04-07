use std::io::Stdout;
use std::io::Write;
use std::sync::Mutex;
use std::sync::OnceLock;

pub use crate::cli::format::format_tool_input;
pub use crate::cli::format::format_tool_input_lines;
pub use crate::cli::format::summarize_inline;
pub use crate::cli::format::truncate;
use crate::protocol::TranscriptItem;
use crate::protocol::UsageSummary;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const RED: &str = "\x1b[31m";
pub const BLACK: &str = "\x1b[30m";
pub const WHITE: &str = "\x1b[37m";
pub const GRAY: &str = "\x1b[90m";
pub const BG_TOOL: &str = "\x1b[48;2;245;197;66m";
pub const BG_OK: &str = "\x1b[48;2;133;220;140m";
pub const BG_ERR: &str = "\x1b[48;2;157;57;57m";

static TERMINAL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn with_terminal<T>(render: impl FnOnce(&mut Stdout) -> T) -> T {
    let _guard = TERMINAL_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut stdout = std::io::stdout();
    let result = render(&mut stdout);
    let _ = stdout.flush();
    result
}

fn write_terminal(text: &str) {
    let normalized = normalize_terminal_newlines(text);
    with_terminal(|stdout| {
        let _ = write!(stdout, "{normalized}");
    });
}

// ---------------------------------------------------------------------------
// Low-level terminal output
// ---------------------------------------------------------------------------

pub fn normalize_terminal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

pub fn terminal_write(text: &str) {
    write_terminal(text);
}

pub fn terminal_writeln(text: &str) {
    terminal_write(text);
    terminal_write("\r\n");
}

pub fn terminal_prefixed_writeln(text: &str) {
    let normalized = normalize_terminal_newlines(text);
    let output = format!("{DIM}•{RESET} {normalized}\r\n");
    write_terminal(&output);
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

pub fn human_duration(duration_ms: u64) -> String {
    if duration_ms >= 1000 {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    } else {
        format!("{duration_ms}ms")
    }
}

pub fn build_run_summary(
    usage: &UsageSummary,
    turn_count: u32,
    duration_ms: u64,
    llm_calls: u32,
    tool_calls: u32,
) -> String {
    let total_tokens = usage.input + usage.output;

    let mut parts = vec![
        format!("run {}", human_duration(duration_ms)),
        format!("turns {}", turn_count),
    ];
    if llm_calls > 0 {
        parts.push(format!("llm {}", llm_calls));
    }
    if tool_calls > 0 {
        parts.push(format!("tools {}", tool_calls));
    }
    parts.push(format!(
        "tokens {} (in {} · out {})",
        total_tokens, usage.input, usage.output
    ));
    let hit_rate = usage.cache_hit_rate();
    if hit_rate > 0.0 {
        parts.push(format!("cache {:.0}%", hit_rate * 100.0));
    }

    parts.join("  ·  ")
}

// ---------------------------------------------------------------------------
// Transcript rendering
// ---------------------------------------------------------------------------

pub fn print_transcript_messages(items: &[TranscriptItem]) {
    for item in items {
        match item {
            TranscriptItem::User { text } => {
                if !text.trim().is_empty() {
                    println!("{YELLOW}> {RESET}{}", text.trim());
                    println!();
                }
            }
            TranscriptItem::Assistant {
                text, tool_calls, ..
            } => {
                if !text.trim().is_empty() {
                    terminal_prefixed_writeln(text.trim());
                    terminal_writeln("");
                }
                for tc in tool_calls {
                    print_tool_call(&tc.name, &tc.input, None);
                }
            }
            TranscriptItem::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                print_tool_result(tool_name, content, *is_error, None);
            }
            _ => {}
        }
    }
}

pub fn print_tool_call(name: &str, input: &serde_json::Value, preview_command: Option<&str>) {
    let (title, lines) = tool_call_message(name, input);
    print_badge_line(&title, false, false);
    for line in lines {
        terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
    }
    if let Some(cmd) = preview_command {
        terminal_writeln(&format!("{GRAY}  ❯ {cmd}{RESET}"));
    }
    terminal_writeln("");
}

pub fn print_tool_result(
    tool_name: &str,
    content: &str,
    is_error: bool,
    tool_call: Option<&ToolCallSummary>,
) {
    let title = if is_error {
        format!("{tool_name} failed")
    } else {
        format!("{tool_name} completed")
    };
    let lines = tool_result_lines(tool_name, content, is_error, tool_call);
    print_badge_line(&title, true, !is_error);
    let color = if is_error { RED } else { GREEN };
    for line in lines {
        terminal_writeln(&format!("{color}  {line}{RESET}"));
    }
    terminal_writeln("");
}

pub fn print_badge_line(title: &str, is_result: bool, ok: bool) {
    let (badge, rest) = split_tool_title(title);
    let (fg, bg) = if is_result {
        if ok {
            (BLACK, BG_OK)
        } else {
            (WHITE, BG_ERR)
        }
    } else {
        (BLACK, BG_TOOL)
    };

    if rest.is_empty() {
        terminal_writeln(&format!("{bg}{fg}{BOLD}[{badge}]{RESET}"));
    } else {
        terminal_writeln(&format!(
            "{bg}{fg}{BOLD}[{badge}]{RESET} {GRAY}{rest}{RESET}"
        ));
    }
}

pub fn tool_call_message(name: &str, input: &serde_json::Value) -> (String, Vec<String>) {
    let lines = format_tool_input_lines(input);
    (format!("{name} call"), lines)
}

pub fn tool_result_line(
    _tool_name: &str,
    content: &str,
    is_error: bool,
    tool_call: Option<&ToolCallSummary>,
) -> String {
    if !is_error {
        if let Some(tc) = tool_call {
            if tc.name.to_lowercase().contains("read") {
                return format!("Result: {}", tc.summary);
            }
        }
    }
    if content.trim().is_empty() {
        if is_error {
            "Result: tool returned an error".into()
        } else {
            "Result: completed".into()
        }
    } else {
        format!("Result: {}", summarize_inline(content, 160))
    }
}

pub fn tool_result_lines(
    tool_name: &str,
    content: &str,
    is_error: bool,
    tool_call: Option<&ToolCallSummary>,
) -> Vec<String> {
    let should_summarize = !is_error
        && tool_call
            .map(|tc| tc.name.to_lowercase().contains("read"))
            .unwrap_or(false);
    if should_summarize {
        return vec![tool_result_line(tool_name, content, is_error, tool_call)];
    }

    let normalized = content.replace("\r\n", "\n");
    if normalized.contains('\n') {
        let trimmed = normalized.trim_end_matches('\n');
        if trimmed.is_empty() {
            return vec![tool_result_line(tool_name, content, is_error, tool_call)];
        }
        return trimmed.split('\n').map(|line| line.to_string()).collect();
    }
    vec![tool_result_line(tool_name, content, is_error, tool_call)]
}

pub fn split_tool_title(title: &str) -> (String, String) {
    let mut parts = title.split_whitespace();
    let badge = parts.next().unwrap_or("TOOL").to_uppercase();
    let rest = parts.collect::<Vec<_>>().join(" ");
    (badge, rest)
}

/// Minimal summary of a tool call used for result display.
pub struct ToolCallSummary {
    pub name: String,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// LLM call message statistics
// ---------------------------------------------------------------------------

/// Per-role counts and estimated tokens for an LLM call's messages.
#[derive(Debug, Default)]
pub struct MessageStats {
    pub user_count: usize,
    pub assistant_count: usize,
    pub tool_result_count: usize,
    pub user_tokens: usize,
    pub assistant_tokens: usize,
    pub tool_result_tokens: usize,
}

impl MessageStats {
    pub fn total_count(&self) -> usize {
        self.user_count + self.assistant_count + self.tool_result_count
    }

    pub fn total_tokens(&self, system_prompt_tokens: usize) -> usize {
        system_prompt_tokens + self.user_tokens + self.assistant_tokens + self.tool_result_tokens
    }
}

/// Count messages by role and estimate tokens from JSON byte size.
pub fn count_messages_by_role(messages: &[serde_json::Value]) -> MessageStats {
    let mut stats = MessageStats::default();
    for msg in messages {
        let role = msg
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let est = msg.to_string().len() / 4;
        match role {
            "user" => {
                stats.user_count += 1;
                stats.user_tokens += est;
            }
            "assistant" => {
                stats.assistant_count += 1;
                stats.assistant_tokens += est;
            }
            "toolResult" | "tool_result" | "tool" => {
                stats.tool_result_count += 1;
                stats.tool_result_tokens += est;
            }
            _ => {
                // Unknown roles count as user
                stats.user_count += 1;
                stats.user_tokens += est;
            }
        }
    }
    stats
}

/// Format the two detail lines for an LLM call badge.
pub fn format_llm_call_lines(
    stats: &MessageStats,
    tool_count: usize,
    system_prompt_tokens: usize,
) -> (String, String) {
    // Line 1: message counts
    let total = stats.total_count();
    let mut role_parts = Vec::new();
    if stats.user_count > 0 {
        role_parts.push(format!("user {}", stats.user_count));
    }
    if stats.assistant_count > 0 {
        role_parts.push(format!("assistant {}", stats.assistant_count));
    }
    if stats.tool_result_count > 0 {
        role_parts.push(format!("tool_result {}", stats.tool_result_count));
    }
    let msg_line = if role_parts.is_empty() {
        format!("{total} messages · {tool_count} tools")
    } else {
        format!(
            "{total} messages ({}) · {tool_count} tools",
            role_parts.join(" · ")
        )
    };

    // Line 2: token estimates by role
    let est_total = stats.total_tokens(system_prompt_tokens);
    let mut token_parts = vec![format!("sys ~{system_prompt_tokens}")];
    if stats.user_tokens > 0 {
        token_parts.push(format!("user ~{}", stats.user_tokens));
    }
    if stats.assistant_tokens > 0 {
        token_parts.push(format!("assistant ~{}", stats.assistant_tokens));
    }
    if stats.tool_result_tokens > 0 {
        token_parts.push(format!("tool_result ~{}", stats.tool_result_tokens));
    }
    let token_line = format!("~{est_total} est tokens ({})", token_parts.join(" · "));

    (msg_line, token_line)
}
