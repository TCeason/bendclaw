//! `/sessions <query>`: the task prompt a normal agent turn expands into.
//!
//! Code owns the deterministic part — the time window, where the archive
//! lives, what a transcript looks like, the answer shape. The agent owns the
//! semantic part with its ordinary tools: list the sessions in the window,
//! grep transcripts with keyword variants, read the matching parts, decide.

use std::path::Path;

/// Lookback when the user gives no window.
pub const DEFAULT_WINDOW_DAYS: u32 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSearch {
    pub query: String,
    /// `None` means the whole archive (`--all`).
    pub window_days: Option<u32>,
}

impl SessionSearch {
    /// Parse the argument string after `/sessions`: `--days N`,
    /// `--since <N>[dwmy]` and `--all` set the window; the rest is the query.
    /// `None` when no query remains or a window value is malformed.
    pub fn parse(args: &str) -> Option<Self> {
        let mut window_days = Some(DEFAULT_WINDOW_DAYS);
        let mut query = Vec::new();
        let mut tokens = args.split_whitespace();
        while let Some(token) = tokens.next() {
            if token == "--all" {
                window_days = None;
                continue;
            }
            let value = match token.split_once('=') {
                Some(("--days" | "--since", value)) => Some(value),
                None if token == "--days" || token == "--since" => tokens.next(),
                _ => {
                    query.push(token);
                    continue;
                }
            };
            window_days = Some(parse_window(value?)?);
        }
        let query = unquote(&query.join(" ")).to_string();
        (!query.is_empty()).then_some(Self { query, window_days })
    }

    pub fn describe_window(&self) -> String {
        match self.window_days {
            None => "all time".to_string(),
            Some(days) => describe_days(days),
        }
    }

    /// The user turn the agent receives.
    pub fn prompt(&self, sessions_dir: &Path, now: chrono::DateTime<chrono::Utc>) -> String {
        let dir = sessions_dir.display();
        let scope = match self.window_days {
            None => "Consider the whole archive.".to_string(),
            Some(days) => {
                let cutoff = now - chrono::Duration::days(i64::from(days));
                format!(
                    "Consider only sessions whose session.json `updated_at` is on or after {} ({}).",
                    cutoff.format("%Y-%m-%dT%H:%M:%SZ"),
                    describe_days(days),
                )
            }
        };
        format!(
            "Find my past sessions that are about: {query}\n\n\
             Archive: {dir}/<session_id>/ holds session.json (metadata: title, custom_title, cwd, \
             source, updated_at) and transcript.jsonl (one JSON object per line; items with \
             \"type\":\"user\" carry my prompts in \"text\", assistant items carry replies and tool calls).\n\
             {scope} Skip sessions whose source is \"automation\". Directory mtimes track updated_at, \
             so listing by mtime is a fast way to find the window.\n\n\
             Method: match on meaning — synonyms, translations, abbreviations and closely related \
             concepts all count. Start from titles, then confirm with grep over the transcripts \
             using several keyword variants, reading only the matching parts. A session that merely \
             mentions a query word in passing is not a hit. Do not modify anything.\n\n\
             Answer with one short summary line, then one line per relevant session, most relevant \
             first, in exactly this form and nothing else after it:\n\
             - <session_id> — <title> — <one-line reason>\n\
             If nothing is relevant, end with exactly: NONE",
            query = self.query,
        )
    }
}

fn parse_window(value: &str) -> Option<u32> {
    let (digits, unit) = match value.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((index, _)) => value.split_at(index),
        None => (value, "d"),
    };
    let amount: u32 = digits.parse().ok().filter(|amount| *amount > 0)?;
    let per_unit = match unit.to_ascii_lowercase().as_str() {
        "d" => 1,
        "w" => 7,
        "m" => 30,
        "y" => 365,
        _ => return None,
    };
    amount.checked_mul(per_unit)
}

fn describe_days(days: u32) -> String {
    let unit = |count: u32, singular: &str, plural: &str| {
        if count == 1 {
            format!("last {singular}")
        } else {
            format!("last {count} {plural}")
        }
    };
    if days.is_multiple_of(365) {
        unit(days / 365, "year", "years")
    } else if days.is_multiple_of(30) {
        unit(days / 30, "month", "months")
    } else if days.is_multiple_of(7) {
        unit(days / 7, "week", "weeks")
    } else {
        unit(days, "day", "days")
    }
}

fn unquote(query: &str) -> &str {
    let query = query.trim();
    for (open, close) in [
        ("'", "'"),
        ("\"", "\""),
        ("\u{2018}", "\u{2019}"),
        ("\u{201c}", "\u{201d}"),
    ] {
        if let Some(inner) = query
            .strip_prefix(open)
            .and_then(|value| value.strip_suffix(close))
        {
            return inner.trim();
        }
    }
    query
}
