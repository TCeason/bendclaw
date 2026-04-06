use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use crossterm::cursor::Hide;
use crossterm::cursor::Show;
use crossterm::execute;

use crate::request::RequestFinishedPayload;
use crate::request::ToolResultPayload;
use crate::storage::model::TranscriptItem;

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
pub const CLEAR_LINE: &str = "\r\x1b[2K\r";

// ---------------------------------------------------------------------------
// Spinner
// ---------------------------------------------------------------------------

pub struct Spinner {
    running: Arc<AtomicBool>,
    message: Arc<Mutex<String>>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    pub fn start(initial_message: &str) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let message = Arc::new(Mutex::new(initial_message.to_string()));
        let running_flag = running.clone();
        let message_ref = message.clone();

        // Hide cursor immediately before spawning the thread
        let _ = execute!(std::io::stdout(), Hide);

        let handle = std::thread::spawn(move || {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut index = 0usize;
            while running_flag.load(Ordering::Relaxed) {
                let label = message_ref
                    .lock()
                    .map(|v| v.clone())
                    .unwrap_or_else(|_| "Working...".into());
                // Render immediately on first frame, then sleep
                print!(
                    "{CLEAR_LINE}{DIM}{} {}  (Press ESC to stop){RESET}",
                    frames[index % frames.len()],
                    label
                );
                let _ = std::io::stdout().flush();
                std::thread::sleep(Duration::from_millis(80));
                index = index.wrapping_add(1);
            }
            print!("{CLEAR_LINE}");
            let _ = std::io::stdout().flush();
        });

        Self {
            running,
            message,
            handle: Some(handle),
        }
    }

    pub fn update(&mut self, message: &str) {
        if let Ok(mut current) = self.message.lock() {
            *current = message.to_string();
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        // Restore cursor after spinner stops
        let _ = execute!(std::io::stdout(), Show);
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// Low-level terminal output
// ---------------------------------------------------------------------------

pub fn normalize_terminal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

pub fn terminal_write(text: &str) {
    let normalized = normalize_terminal_newlines(text);
    print!("{normalized}");
    let _ = std::io::stdout().flush();
}

pub fn terminal_writeln(text: &str) {
    terminal_write(text);
    terminal_write("\r\n");
}

pub fn terminal_message_prefix() {
    terminal_write(&format!("{DIM}•{RESET} "));
}

pub fn terminal_prefixed_writeln(text: &str) {
    terminal_message_prefix();
    terminal_writeln(text);
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut value: String = s.chars().take(max).collect();
        value.push_str("...");
        value
    }
}

pub const SUMMARY_KEYS: &[&str] = &[
    "file_path",
    "path",
    "command",
    "pattern",
    "patterns",
    "query",
    "url",
    "name",
    "directory",
    "glob",
    "regex",
];

pub fn format_tool_input(input: &serde_json::Value) -> String {
    if let Some(obj) = input.as_object() {
        for &key in SUMMARY_KEYS {
            if let Some(val) = obj.get(key) {
                if let Some(s) = val.as_str() {
                    return summarize_inline(s, 100);
                }
                if let Some(arr) = val.as_array() {
                    let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                    if !parts.is_empty() {
                        return summarize_inline(&parts.join(", "), 100);
                    }
                }
            }
        }
    }
    summarize_inline(&input.to_string(), 100)
}

pub fn summarize_inline(value: &str, max_chars: usize) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, max_chars)
}

pub fn human_duration(duration_ms: u64) -> String {
    if duration_ms >= 1000 {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    } else {
        format!("{duration_ms}ms")
    }
}

pub fn build_run_summary(payload: &RequestFinishedPayload) -> String {
    let total_tokens = payload
        .usage
        .get("input")
        .and_then(|v| v.as_u64())
        .unwrap_or_default()
        + payload
            .usage
            .get("output")
            .and_then(|v| v.as_u64())
            .unwrap_or_default();

    [
        format!("run {}", human_duration(payload.duration_ms)),
        format!("turns {}", payload.turn_count),
        format!("tokens {}", total_tokens),
    ]
    .join("  ·  ")
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
                    print_tool_call(&tc.name, &tc.input);
                }
            }
            TranscriptItem::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                let title = if *is_error {
                    format!("{tool_name} failed")
                } else {
                    format!("{tool_name} completed")
                };
                print_badge_line(&title, true, !is_error);
                terminal_writeln(&format!(
                    "{}  {}{}",
                    if *is_error { RED } else { GREEN },
                    summarize_inline(content, 160),
                    RESET
                ));
                terminal_writeln("");
            }
            _ => {}
        }
    }
}

pub fn print_tool_call(name: &str, input: &serde_json::Value) {
    let (title, lines) = tool_call_message(name, input);
    print_badge_line(&title, false, false);
    for line in lines {
        terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
    }
    terminal_writeln("");
}

pub fn print_tool_result(payload: &ToolResultPayload, tool_call: Option<&ToolCallSummary>) {
    let title = tool_result_title(payload);
    let line = tool_result_line(payload, tool_call);
    print_badge_line(&title, true, !payload.is_error);
    terminal_writeln(&format!(
        "{}  {}{}",
        if payload.is_error { RED } else { GREEN },
        line,
        RESET
    ));
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
    let lowercase = name.to_lowercase();
    if lowercase.contains("grep") {
        return ("Grep 1 search".into(), vec![format!(
            "\"{}\"",
            format_tool_input(input)
        )]);
    }
    if lowercase.contains("glob") {
        return ("Glob 1 pattern".into(), vec![format_tool_input(input)]);
    }
    if lowercase.contains("read") {
        return ("Read 1 file".into(), vec![format_tool_input(input)]);
    }
    (format!("{name} call"), vec![format_tool_input(input)])
}

pub fn tool_result_title(payload: &ToolResultPayload) -> String {
    if payload.is_error {
        format!("{} failed", payload.tool_name)
    } else {
        format!("{} completed", payload.tool_name)
    }
}

pub fn tool_result_line(
    payload: &ToolResultPayload,
    tool_call: Option<&ToolCallSummary>,
) -> String {
    if !payload.is_error {
        if let Some(tc) = tool_call {
            if tc.name.to_lowercase().contains("read") {
                return format!("Result: {}", tc.summary);
            }
        }
    }
    if payload.content.trim().is_empty() {
        if payload.is_error {
            "Result: tool returned an error".into()
        } else {
            "Result: completed".into()
        }
    } else {
        format!("Result: {}", summarize_inline(&payload.content, 160))
    }
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
