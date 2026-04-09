//! GitHub issue reference auto-linking.

use super::theme::Style;

/// Replace `owner/repo#123` patterns with OSC 8 clickable hyperlinks.
pub fn linkify_issue_refs(input: &str, link_style: &Style) -> String {
    let mut out = String::new();
    let mut cursor = 0usize;

    while cursor < input.len() {
        let Some((start, end, repo, num)) = find_issue_ref(input, cursor) else {
            out.push_str(&input[cursor..]);
            break;
        };
        out.push_str(&input[cursor..start]);
        let display = format!("{}#{}", repo, num);
        out.push_str(&format_hyperlink(
            &format!("https://github.com/{repo}/issues/{num}"),
            &link_style.paint(&display),
            None,
        ));
        cursor = end;
    }

    out
}

/// Format an OSC 8 terminal hyperlink.
pub fn format_hyperlink(url: &str, text: &str, fallback_url: Option<&str>) -> String {
    match fallback_url {
        Some(fallback) => format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\ ({fallback})"),
        None => format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\"),
    }
}

fn find_issue_ref(input: &str, offset: usize) -> Option<(usize, usize, String, String)> {
    let slice = &input[offset..];
    for (rel_index, ch) in slice.char_indices() {
        if ch != '#' {
            continue;
        }

        let hash_index = offset + rel_index;
        let after_hash = &input[hash_index + 1..];
        let num_end = after_hash
            .char_indices()
            .find(|(_, c)| !c.is_ascii_digit())
            .map(|(idx, _)| idx)
            .unwrap_or(after_hash.len());
        if num_end == 0 {
            continue;
        }
        let num = &after_hash[..num_end];

        let before_hash = &input[..hash_index];
        let repo_start = before_hash
            .char_indices()
            .rev()
            .find(|(_, c)| !is_issue_repo_char(*c))
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(0);
        let repo = &input[repo_start..hash_index];
        if !is_valid_issue_repo(repo) {
            continue;
        }

        if repo_start > 0 {
            let prefix = input[..repo_start].chars().last();
            if let Some(prefix_char) = prefix {
                if prefix_char.is_ascii_alphanumeric()
                    || prefix_char == '_'
                    || prefix_char == '.'
                    || prefix_char == '/'
                    || prefix_char == '-'
                {
                    continue;
                }
            }
        }

        let end = hash_index + 1 + num_end;
        return Some((repo_start, end, repo.to_string(), num.to_string()));
    }
    None
}

fn is_issue_repo_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '/' || ch == '.'
}

fn is_valid_issue_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(name) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    is_valid_owner(owner) && is_valid_repo_name(name)
}

fn is_valid_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn is_valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}
