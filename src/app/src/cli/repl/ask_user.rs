//! Terminal UI for the `ask_user` tool — renders structured questions and
//! collects user input via keyboard navigation.
//!
//! Pure rendering functions (`build_*`) are separated from IO functions
//! (`render_and_select`) for testability.

use std::io::Write;

use bend_engine::tools::AskUserRequest;
use bend_engine::tools::AskUserResponse;
use crossterm::event::poll;
use crossterm::event::read;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;

use super::render::with_terminal;
use super::render::DIM;
use super::render::GREEN;
use super::render::RESET;
use super::render::YELLOW;

// ---------------------------------------------------------------------------
// ANSI helpers
// ---------------------------------------------------------------------------

const ERASE_LINE: &str = "\x1b[K";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";

fn cursor_up(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("\x1b[{n}A")
    }
}

// ---------------------------------------------------------------------------
// Pure rendering (testable without a terminal)
// ---------------------------------------------------------------------------

/// Total option count including the fixed "None of the above" entry.
fn total_options(request: &AskUserRequest) -> usize {
    request.options.len() + 1
}

/// Build the full question block as a string. Returns `(output, line_count)`.
///
/// Layout:
/// ```text
///   ❓ <question>
///                          ← blank line
///   › 1. Label (Recommended)
///        Description text
///     2. Another option
///        Description text
///     0. None of the above (type your own)
///                          ← blank line
///   [↑↓ select  Enter confirm  1-N pick  0 custom  Esc skip]
/// ```
pub fn build_question_block(request: &AskUserRequest, selected: usize) -> (String, usize) {
    let mut out = String::new();
    let mut lines: usize = 0;

    // Question line
    out.push_str(&format!(
        "\r{ERASE_LINE}  {CYAN}❓ {BOLD}{}{RESET}\r\n",
        request.question
    ));
    lines += 1;

    // Blank line
    out.push_str(&format!("{ERASE_LINE}\r\n"));
    lines += 1;

    // Numbered options
    for (i, opt) in request.options.iter().enumerate() {
        let num = i + 1;
        let is_selected = i == selected;
        let marker = if is_selected { "›" } else { " " };
        let highlight = if is_selected { YELLOW } else { DIM };

        out.push_str(&format!(
            "{ERASE_LINE}  {highlight}{marker} {num}. {}{RESET}\r\n",
            opt.label
        ));
        lines += 1;

        out.push_str(&format!(
            "{ERASE_LINE}  {DIM}     {}{RESET}\r\n",
            opt.description
        ));
        lines += 1;
    }

    // "None of the above" option
    let none_idx = request.options.len();
    let is_none_selected = selected == none_idx;
    let marker = if is_none_selected { "›" } else { " " };
    let highlight = if is_none_selected { YELLOW } else { DIM };
    out.push_str(&format!(
        "{ERASE_LINE}  {highlight}{marker} 0. None of the above (type your own){RESET}\r\n",
    ));
    lines += 1;

    // Blank line
    out.push_str(&format!("{ERASE_LINE}\r\n"));
    lines += 1;

    // Footer hint (with \r\n so cursor lands on the line below the block)
    out.push_str(&format!(
        "{ERASE_LINE}  {DIM}[↑↓ select  Enter confirm  1-{} pick  0 custom  Esc skip]{RESET}\r\n",
        request.options.len()
    ));
    lines += 1;

    (out, lines)
}

/// Build the confirmation line shown after the user selects an option.
pub fn build_confirmation(label: &str) -> String {
    format!("  {GREEN}✓ {label}{RESET}")
}

/// Build the skip line shown when the user presses Esc.
pub fn build_skipped() -> String {
    format!("  {DIM}— skipped{RESET}")
}

// ---------------------------------------------------------------------------
// Terminal interaction
// ---------------------------------------------------------------------------

/// Result from the ask_user UI interaction.
pub enum AskUserUiResult {
    /// User provided an answer (selected, custom, or skipped).
    Answer(AskUserResponse),
    /// User pressed Ctrl+C — caller should abort the entire run.
    ExitRun,
}

/// Render the question selector in the current raw-mode terminal and wait
/// for the user to pick an option, type custom input, or skip.
///
/// Caller must already be in raw mode (via `RawModeGuard`).
pub fn render_and_select(request: &AskUserRequest) -> std::io::Result<AskUserUiResult> {
    let total = total_options(request);
    let mut selected: usize = 0;
    let mut prev_lines: usize = 0;
    let mut needs_redraw = true;

    loop {
        if needs_redraw {
            let (output, line_count) = build_question_block(request, selected);
            with_terminal(|stdout| {
                // Cursor is on the line below the block; move up to line 1
                if prev_lines > 0 {
                    let _ = write!(stdout, "\r{}", cursor_up(prev_lines));
                }
                let _ = write!(stdout, "{output}");
                let _ = stdout.flush();
            });
            prev_lines = line_count;
            needs_redraw = false;
        }

        // Wait for key
        if !poll(std::time::Duration::from_millis(100))? {
            continue;
        }

        match read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                // Navigation
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected > 0 {
                        selected -= 1;
                    } else {
                        selected = total - 1;
                    }
                    needs_redraw = true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1) % total;
                    needs_redraw = true;
                }

                // Confirm current selection
                KeyCode::Enter => {
                    let response = if selected < request.options.len() {
                        let label = request.options[selected].label.clone();
                        clear_block(prev_lines);
                        print_result(&build_confirmation(&label));
                        AskUserResponse::Selected(label)
                    } else {
                        // "None of the above" → custom input
                        clear_block(prev_lines);
                        match read_custom_input()? {
                            Some(text) => {
                                print_result(&build_confirmation(&text));
                                AskUserResponse::Custom(text)
                            }
                            None => {
                                print_result(&build_skipped());
                                AskUserResponse::Skipped
                            }
                        }
                    };
                    return Ok(AskUserUiResult::Answer(response));
                }

                // Quick-pick by number (1-N for options, 0 for custom)
                KeyCode::Char(ch @ '1'..='9') => {
                    let idx = (ch as usize) - ('1' as usize);
                    if idx < request.options.len() {
                        let label = request.options[idx].label.clone();
                        clear_block(prev_lines);
                        print_result(&build_confirmation(&label));
                        return Ok(AskUserUiResult::Answer(AskUserResponse::Selected(label)));
                    }
                }
                KeyCode::Char('0') => {
                    clear_block(prev_lines);
                    match read_custom_input()? {
                        Some(text) => {
                            print_result(&build_confirmation(&text));
                            return Ok(AskUserUiResult::Answer(AskUserResponse::Custom(text)));
                        }
                        None => {
                            print_result(&build_skipped());
                            return Ok(AskUserUiResult::Answer(AskUserResponse::Skipped));
                        }
                    }
                }

                // Ctrl+C — abort the entire run
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    clear_block(prev_lines);
                    return Ok(AskUserUiResult::ExitRun);
                }

                // Skip
                KeyCode::Esc => {
                    clear_block(prev_lines);
                    print_result(&build_skipped());
                    return Ok(AskUserUiResult::Answer(AskUserResponse::Skipped));
                }

                _ => {}
            },
            _ => {}
        }
    }
}

/// Clear the rendered question block.
/// Cursor starts on the line below the block (all lines end with \r\n).
fn clear_block(line_count: usize) {
    if line_count == 0 {
        return;
    }
    with_terminal(|stdout| {
        // Move to the first line of the block
        let _ = write!(stdout, "\r{}", cursor_up(line_count));
        // Erase each line
        for _ in 0..line_count {
            let _ = write!(stdout, "{ERASE_LINE}\r\n");
        }
        // Move back to the top so the next output starts at the right place
        let _ = write!(stdout, "\r{}", cursor_up(line_count));
        let _ = stdout.flush();
    });
}

/// Print a single result line after the question is resolved.
fn print_result(text: &str) {
    with_terminal(|stdout| {
        let _ = write!(stdout, "{text}\r\n\r\n");
        let _ = stdout.flush();
    });
}

/// Temporarily exit raw mode to read a line of free-form text.
/// Returns `None` if the user enters an empty string.
fn read_custom_input() -> std::io::Result<Option<String>> {
    let input = with_line_input(|| {
        with_terminal(|stdout| {
            let _ = write!(stdout, "  {YELLOW}> {RESET}");
        });
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(buf)
    })?;

    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

/// Suspend raw mode, run a closure that needs normal line editing, then
/// restore raw mode. Errors from mode switching are propagated.
fn with_line_input<T>(f: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
    disable_raw_mode()?;
    let result = f();
    enable_raw_mode()?;
    result
}
