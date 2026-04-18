//! Gateway command parsing for `/clear`, `/goto`, etc.

// ---------------------------------------------------------------------------
// Command — parsed gateway commands
// ---------------------------------------------------------------------------

pub enum Command {
    Clear,
    Goto(u64),
    History(usize),
    UsageError(String),
}

pub fn parse_command(text: &str) -> Option<Command> {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();

    if lower == "/clear" {
        return Some(Command::Clear);
    }
    if lower == "/goto" || lower.starts_with("/goto ") {
        let arg = lower.strip_prefix("/goto").map(|s| s.trim());
        match arg {
            Some(s) if !s.is_empty() => match s.parse::<u64>() {
                Ok(seq) if seq > 0 => return Some(Command::Goto(seq)),
                _ => return Some(Command::UsageError("Usage: /goto <message_number>".into())),
            },
            _ => return Some(Command::UsageError("Usage: /goto <message_number>".into())),
        }
    }
    if lower == "/history" || lower.starts_with("/history ") {
        let arg = lower.strip_prefix("/history").map(|s| s.trim());
        match arg {
            Some(s) if !s.is_empty() => match s.parse::<usize>() {
                Ok(n) if n > 0 => return Some(Command::History(n)),
                _ => return Some(Command::UsageError("Usage: /history [count]".into())),
            },
            _ => return Some(Command::History(20)),
        }
    }
    None
}
