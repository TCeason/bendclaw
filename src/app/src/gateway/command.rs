//! Gateway command parsing for `/clear`, `/goto`, etc.

// ---------------------------------------------------------------------------
// Command — parsed gateway commands
// ---------------------------------------------------------------------------

pub enum Command {
    Clear,
    Goto(u64),
    History(usize),
    Goal(GoalCommand),
    UsageError(String),
}

/// Subcommands for `/goal …`.
pub enum GoalCommand {
    /// `/goal` or `/goal show` — print current goal state.
    Show,
    /// `/goal <condition> [--budget=<tokens>] [--max-iter=<n>] [--timeout=<secs>]`.
    Set {
        condition: String,
        max_tokens: Option<u64>,
        max_iterations: Option<u32>,
        max_seconds: Option<u64>,
    },
    /// `/goal pause`.
    Pause,
    /// `/goal resume`.
    Resume,
    /// `/goal clear`.
    Clear,
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
    if lower == "/goal" || lower.starts_with("/goal ") {
        return Some(parse_goal(trimmed));
    }
    None
}

// ---------------------------------------------------------------------------
// /goal parser
// ---------------------------------------------------------------------------

const GOAL_USAGE: &str =
    "Usage: /goal <condition> [--budget=<tokens>] [--max-iter=<n>] [--timeout=<secs>]\n       /goal [show|pause|resume|clear]";

fn parse_goal(trimmed: &str) -> Command {
    let rest = trimmed
        .strip_prefix("/goal")
        .or_else(|| trimmed.strip_prefix("/GOAL"))
        .map(str::trim_start)
        .unwrap_or("");

    if rest.is_empty() {
        return Command::Goal(GoalCommand::Show);
    }

    // First whitespace-separated token is the subcommand (if it matches a known one).
    let (head, tail) = match rest.split_once(char::is_whitespace) {
        Some((h, t)) => (h, t.trim_start()),
        None => (rest, ""),
    };

    match head.to_ascii_lowercase().as_str() {
        "show" if tail.is_empty() => Command::Goal(GoalCommand::Show),
        "pause" if tail.is_empty() => Command::Goal(GoalCommand::Pause),
        "resume" if tail.is_empty() => Command::Goal(GoalCommand::Resume),
        "clear" | "stop" | "off" | "reset" | "none" | "cancel" if tail.is_empty() => {
            Command::Goal(GoalCommand::Clear)
        }
        // Treat everything else as the condition text (implicit set).
        _ if !head.starts_with("--") => parse_goal_set(rest),
        _ => Command::UsageError(GOAL_USAGE.into()),
    }
}

fn parse_goal_set(args: &str) -> Command {
    if args.trim().is_empty() {
        return Command::UsageError(GOAL_USAGE.into());
    }
    let mut max_tokens: Option<u64> = None;
    let mut max_iterations: Option<u32> = None;
    let mut max_seconds: Option<u64> = None;
    let mut condition_parts: Vec<&str> = Vec::new();

    for part in args.split_whitespace() {
        if let Some(value) = part.strip_prefix("--budget=") {
            match value.parse::<u64>() {
                Ok(n) if n > 0 => max_tokens = Some(n),
                _ => {
                    return Command::UsageError(format!(
                        "invalid --budget value '{value}' (expected positive integer)"
                    ));
                }
            }
        } else if let Some(value) = part.strip_prefix("--max-iter=") {
            match value.parse::<u32>() {
                Ok(n) if n > 0 => max_iterations = Some(n),
                _ => {
                    return Command::UsageError(format!(
                        "invalid --max-iter value '{value}' (expected positive integer)"
                    ));
                }
            }
        } else if let Some(value) = part.strip_prefix("--timeout=") {
            match value.parse::<u64>() {
                Ok(n) if n > 0 => max_seconds = Some(n),
                _ => {
                    return Command::UsageError(format!(
                        "invalid --timeout value '{value}' (expected positive integer)"
                    ));
                }
            }
        } else {
            condition_parts.push(part);
        }
    }

    let condition = condition_parts.join(" ").trim().to_string();
    if condition.is_empty() {
        return Command::UsageError(GOAL_USAGE.into());
    }
    Command::Goal(GoalCommand::Set {
        condition,
        max_tokens,
        max_iterations,
        max_seconds,
    })
}
