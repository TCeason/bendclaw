use std::borrow::Cow;

pub const SYSTEM_PROMPT_DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

pub(crate) fn split_dynamic(prompt: &str) -> (&str, &str) {
    match prompt.rsplit_once(SYSTEM_PROMPT_DYNAMIC_BOUNDARY) {
        Some((static_part, dynamic_part)) => (static_part.trim_end(), dynamic_part.trim_start()),
        None => (prompt, ""),
    }
}

/// Remove evot's internal cache boundary before sending a plain-text system
/// prompt to providers that do not support separate static and dynamic blocks.
pub(crate) fn without_dynamic_boundary(prompt: &str) -> Cow<'_, str> {
    let Some((static_part, dynamic_part)) = prompt.rsplit_once(SYSTEM_PROMPT_DYNAMIC_BOUNDARY)
    else {
        return Cow::Borrowed(prompt);
    };
    let static_part = static_part.trim_end();
    let dynamic_part = dynamic_part.trim_start();
    match (static_part.is_empty(), dynamic_part.is_empty()) {
        (false, false) => Cow::Owned(format!("{static_part}\n\n{dynamic_part}")),
        (false, true) => Cow::Owned(static_part.to_string()),
        (true, false) => Cow::Owned(dynamic_part.to_string()),
        (true, true) => Cow::Owned(String::new()),
    }
}
