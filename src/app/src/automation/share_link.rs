//! Recognise a shared-task link without trusting it.
//!
//! `/task <something>` has to tell a share link from a natural-language
//! request, and it must not turn evot into a fetcher of arbitrary URLs. So
//! only the path is read: `/share/t/<id>` on any host, or a bare id. The
//! snapshot is then fetched from this client's own server, never from the
//! host the user pasted.

/// Server ids are `token_urlsafe(16)`: 22 URL-safe base64 characters.
const ID_LEN: usize = 22;

fn is_share_id(candidate: &str) -> bool {
    candidate.len() == ID_LEN
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The share id inside `input`, when `input` is a task share link or id.
///
/// Accepts `https://host/share/t/<id>`, with or without a scheme, trailing
/// slash, `/task.json`, query or fragment; and `<id>` alone. Anything else —
/// a session share, a prompt, a prompt that happens to mention a URL — is
/// `None`, and `/task` treats it as a request.
pub fn parse_task_share_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return None;
    }
    if is_share_id(trimmed) {
        return Some(trimmed.to_string());
    }
    let without_scheme = trimmed
        .split_once("://")
        .map(|(scheme, rest)| {
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
                rest
            } else {
                ""
            }
        })
        .unwrap_or(trimmed);
    let path_start = without_scheme.find('/')?;
    let path = &without_scheme[path_start..];
    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    let rest = path.strip_prefix("/share/t/")?;
    let id = rest.strip_suffix("/task.json").unwrap_or(rest);
    is_share_id(id).then(|| id.to_string())
}
