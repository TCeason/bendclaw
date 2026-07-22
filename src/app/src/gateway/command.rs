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
    /// `/mem` — archive durable knowledge from the current conversation into
    /// the memory vault. `/mem <terms>` — search the vault for entries
    /// relevant to the terms. Both need the LLM (distilling / fuzzy matching):
    /// they expand into normal prompts (see [`memorize_prompt`] and
    /// [`recall_prompt`]) instead of being handled directly.
    Memorize,
    MemorySearch {
        query: String,
    },
    /// Hidden `/_rsearch <query>` — semantic session search for `/resume`.
    /// Handled directly: ranks recent sessions against the query with a
    /// one-shot LLM call and returns the list as a command outcome.
    ResumeSearch {
        query: String,
    },
    UsageError(String),
}

/// Build the prompt a bare `/mem` command expands into.
pub fn memorize_prompt() -> String {
    "Activate the `memory` skill and archive the durable knowledge from \
     this conversation into the memory vault, following the skill's \
     archive workflow."
        .to_string()
}

/// Build the prompt a `/mem <terms>` command expands into.
pub fn recall_prompt(query: &str) -> String {
    format!(
        "Activate the `memory` skill and search the memory vault for entries \
         relevant to: {query}\n\n\
         Match liberally — consider synonyms, related terms, and partial \
         matches, not just exact keywords. List each matching entry as its \
         absolute .md file path with a one-line description, then briefly \
         summarize the most relevant entry. If nothing matches, say so."
    )
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
    if lower == "/mem" || lower.starts_with("/mem ") {
        let arg = trimmed.get("/mem".len()..).map(str::trim).unwrap_or("");
        return Some(if arg.is_empty() {
            Command::Memorize
        } else {
            Command::MemorySearch {
                query: arg.to_string(),
            }
        });
    }
    if lower == "/_rsearch" || lower.starts_with("/_rsearch ") {
        let arg = trimmed
            .get("/_rsearch".len()..)
            .map(str::trim)
            .unwrap_or("");
        return Some(if arg.is_empty() {
            Command::UsageError("Usage: /_rsearch <query>".to_string())
        } else {
            Command::ResumeSearch {
                query: arg.to_string(),
            }
        });
    }
    None
}
