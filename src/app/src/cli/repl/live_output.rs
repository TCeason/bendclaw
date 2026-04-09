//! LiveOutput — a reusable multi-line terminal region that refreshes in-place.
//!
//! Instead of appending new lines on every update, `LiveOutput` uses ANSI cursor
//! control sequences to overwrite the previously rendered block. This gives a
//! smooth "dynamic refresh" effect for long-running output like bash commands,
//! compaction progress, or any streaming content.
//!
//! # Usage
//!
//! ```ignore
//! let mut live = LiveOutput::new();
//! live.update(&["Downloading… 42%", "  langchain-openai-1.1.11"]);
//! // later:
//! live.update(&["Downloading… 78%", "  langchain-core-0.3.50"]);
//! // when done:
//! live.clear();
//! ```

use std::io::Write;

use super::render::with_terminal;

/// Maximum characters per line before truncation.
/// Prevents terminal auto-wrap from breaking the line count.
const MAX_LINE_WIDTH: usize = 120;

/// A multi-line terminal region that can be refreshed in-place.
///
/// Each call to [`update`](LiveOutput::update) overwrites the previous content
/// using ANSI cursor-up sequences. Call [`clear`](LiveOutput::clear) to erase
/// the region when done.
pub struct LiveOutput {
    /// Number of lines rendered in the last `update()` call.
    rendered_lines: usize,
}

impl Default for LiveOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveOutput {
    pub fn new() -> Self {
        Self { rendered_lines: 0 }
    }

    /// Refresh the live region with new content.
    ///
    /// - If content was previously rendered, cursor moves up to overwrite it.
    /// - Each line is truncated to [`MAX_LINE_WIDTH`] to prevent terminal wrap.
    /// - Extra lines from the previous render are cleared.
    pub fn update(&mut self, lines: &[&str]) {
        if lines.is_empty() {
            return;
        }

        with_terminal(|stdout| {
            // Move cursor up to the start of the previously rendered block
            if self.rendered_lines > 0 {
                let _ = write!(stdout, "\x1b[{}A", self.rendered_lines);
            }

            // Render each new line, clearing to end of line
            for line in lines {
                let truncated = truncate_line(line, MAX_LINE_WIDTH);
                let _ = write!(stdout, "\r{truncated}\x1b[K\n");
            }

            // If previous render had more lines, clear the extras
            if lines.len() < self.rendered_lines {
                for _ in 0..(self.rendered_lines - lines.len()) {
                    let _ = write!(stdout, "\r\x1b[K\n");
                }
                // Move cursor back up past the blank lines we just wrote
                let extra = self.rendered_lines - lines.len();
                let _ = write!(stdout, "\x1b[{}A", extra);
            }

            self.rendered_lines = lines.len();
        });
    }

    /// Clear the entire live region and reset state.
    pub fn clear(&mut self) {
        if self.rendered_lines == 0 {
            return;
        }

        with_terminal(|stdout| {
            // Move up to the start of the block
            let _ = write!(stdout, "\x1b[{}A", self.rendered_lines);
            // Clear each line
            for _ in 0..self.rendered_lines {
                let _ = write!(stdout, "\r\x1b[K\n");
            }
            // Move back up so cursor is at the start
            let _ = write!(stdout, "\x1b[{}A", self.rendered_lines);
        });

        self.rendered_lines = 0;
    }

    /// Whether content is currently displayed.
    pub fn is_active(&self) -> bool {
        self.rendered_lines > 0
    }

    /// Reset internal state without clearing the terminal.
    /// Use when the spinner is being fully reactivated or deactivated
    /// and the terminal content is already handled elsewhere.
    pub fn reset(&mut self) {
        self.rendered_lines = 0;
    }

    /// Number of lines currently rendered.
    pub fn line_count(&self) -> usize {
        self.rendered_lines
    }
}

/// Truncate a line to `max_width` characters to prevent terminal auto-wrap.
fn truncate_line(line: &str, max_width: usize) -> &str {
    if line.len() <= max_width {
        return line;
    }
    // Find a safe char boundary
    let mut end = max_width;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}
