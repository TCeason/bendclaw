//! Gateway command parsing for `/clear`, `/compact`, etc.

// ---------------------------------------------------------------------------
// Command — parsed gateway commands
// ---------------------------------------------------------------------------

pub enum Command {
    Clear,
    Compact {
        custom_instructions: Option<String>,
    },
    /// Hidden `/_dump` — emit current system prompt + tools as JSON.
    /// Optional argument is an output path. When None, the agent picks a
    /// timestamped default under `~/.evotai/dumps/`.
    Dump {
        target: Option<String>,
    },
    UsageError(String),
}

pub fn parse_command(text: &str) -> Option<Command> {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();

    if lower == "/clear" {
        return Some(Command::Clear);
    }
    if lower == "/compact" || lower.starts_with("/compact ") {
        let arg = trimmed
            .strip_prefix("/compact")
            .or_else(|| trimmed.strip_prefix("/COMPACT"))
            .map(str::trim)
            .unwrap_or("");
        return Some(Command::Compact {
            custom_instructions: (!arg.is_empty()).then(|| arg.to_string()),
        });
    }
    if lower == "/_dump" || lower.starts_with("/_dump ") {
        let arg = trimmed
            .strip_prefix("/_dump")
            .or_else(|| trimmed.strip_prefix("/_DUMP"))
            .map(str::trim)
            .unwrap_or("");
        let target = (!arg.is_empty()).then(|| arg.to_string());
        return Some(Command::Dump { target });
    }
    None
}
