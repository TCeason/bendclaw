use super::TextMatcher;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;
use crate::types::TranscriptItem;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    pub session: SessionMeta,
    pub matched_field: String,
    pub snippet: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionWithText {
    #[serde(flatten)]
    pub session: SessionMeta,
    pub search_text: String,
}

pub struct SessionSearcher {
    matcher: TextMatcher,
}

impl SessionSearcher {
    pub fn new(query: &str) -> Self {
        Self {
            matcher: TextMatcher::new(query.trim()),
        }
    }

    pub fn matches_meta(&self, session: &SessionMeta) -> Option<SearchHit> {
        if self.matcher.is_empty() {
            return Some(hit(session, "all", ""));
        }

        let fields = [
            ("title", session.title.as_deref().unwrap_or("")),
            ("cwd", &session.cwd),
            ("source", &session.source),
            ("model", &session.model),
            ("session_id", &session.session_id),
        ];

        for (name, value) in &fields {
            if self.matcher.is_substring(value) {
                return Some(hit(session, name, value));
            }
        }
        None
    }

    pub fn matches_transcript(
        &self,
        session: &SessionMeta,
        entries: &[TranscriptEntry],
    ) -> Option<SearchHit> {
        if self.matcher.is_empty() {
            return None;
        }

        for entry in entries {
            if let Some(text) = extract_text(&entry.item) {
                if self.matcher.matches(text) {
                    let snippet = truncate(text, 120);
                    return Some(hit(session, "content", &snippet));
                }
            }
        }
        None
    }
}

/// Max characters of combined transcript body included per session in the
/// flat `search_text`. Keeps the `/api/sessions` payload bounded while still
/// carrying enough message content for keyword search and snippets. Metadata
/// (id/title/cwd/source/model) is always included on top of this budget.
const TRANSCRIPT_TEXT_BUDGET: usize = 6000;

pub fn collect_search_text(session: &SessionMeta, entries: &[TranscriptEntry]) -> String {
    let mut parts = Vec::new();
    parts.push(session.session_id.clone());
    if let Some(t) = &session.title {
        parts.push(t.clone());
    }
    parts.push(session.cwd.clone());
    parts.push(session.source.clone());
    parts.push(session.model.clone());

    // Flatten whole-message text (not just the first line) so keywords buried
    // in multi-line content are searchable and snippets can center on the
    // real hit. Budgeted so one long session can't bloat the response.
    let mut remaining = TRANSCRIPT_TEXT_BUDGET;
    for entry in entries {
        if remaining == 0 {
            break;
        }
        if let Some(text) = extract_text(&entry.item) {
            let normalized = normalize_ws(text);
            if normalized.is_empty() {
                continue;
            }
            let clipped = clip_chars(&normalized, remaining);
            remaining = remaining.saturating_sub(clipped.chars().count());
            parts.push(clipped);
        }
    }
    parts.join(" ")
}

/// Collapse every run of whitespace (including newlines) into a single space
/// and trim the ends, so multi-line message bodies become one searchable line.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Keep at most `max` characters, cutting on a char boundary so we never split
/// a multi-byte (e.g. CJK) character.
fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    s[..end].to_string()
}

fn hit(session: &SessionMeta, field: &str, snippet: &str) -> SearchHit {
    SearchHit {
        session: session.clone(),
        matched_field: field.to_string(),
        snippet: snippet.to_string(),
    }
}

fn extract_text(item: &TranscriptItem) -> Option<&str> {
    match item {
        TranscriptItem::User { text, .. } => Some(text),
        TranscriptItem::Assistant { text, .. } => Some(text),
        TranscriptItem::ToolResult { content, .. } => Some(content),
        TranscriptItem::System { text } => Some(text),
        _ => None,
    }
}

fn truncate(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.chars().count() <= max {
        first_line.to_string()
    } else {
        let end: usize = first_line
            .char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(first_line.len());
        format!("{}…", &first_line[..end])
    }
}
