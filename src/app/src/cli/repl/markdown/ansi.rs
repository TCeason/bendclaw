//! ANSI escape code utilities.

use unicode_width::UnicodeWidthStr;

/// Strip ANSI escape sequences (CSI and OSC) from a string.
pub fn strip_ansi_codes(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            out.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('[') => {
                let _ = chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let _ = chars.next();
                loop {
                    match chars.next() {
                        Some('\x1b') if matches!(chars.peek(), Some('\\')) => {
                            let _ = chars.next();
                            break;
                        }
                        Some('\u{7}') => break,
                        Some(_) => {}
                        None => break,
                    }
                }
            }
            _ => {}
        }
    }

    out
}

/// Visible display width of a string, ignoring ANSI escape codes.
pub fn display_width(input: &str) -> usize {
    UnicodeWidthStr::width(strip_ansi_codes(input).as_str())
}
