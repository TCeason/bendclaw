/// Try to parse JSON, repairing malformed input only when it looks like JSON.
///
/// LLMs sometimes produce truncated or slightly invalid JSON for tool call
/// arguments (trailing commas, missing closing braces, etc.). This function
/// attempts a standard parse first, and only falls back to repair when the
/// input starts with `{`, `[`, or `"` — i.e., it actually looks like JSON.
///
/// Non-JSON strings (plain text, markdown, etc.) are never "repaired" into
/// JSON — the original parse error is returned instead.
///
/// Repair is also rejected when it changes the top-level shape of the value.
/// `jsonrepair` wraps concatenated top-level values (`{..}{..}`) into an
/// array, which turns "several tool calls' arguments landed in one buffer"
/// into a plausible-looking array that later fails schema validation with a
/// misleading "do not wrap arguments in an array" message. Input that starts
/// with `{` must repair to an object, `[` to an array, `"` to a string;
/// anything else is treated as unrecoverable and the original parse error is
/// returned so callers fall back to their empty-object default.
pub fn try_repair_json(raw: &str) -> Result<serde_json::Value, serde_json::Error> {
    // Fast path: standard parse
    let err = match serde_json::from_str(raw) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };

    let Some(lead) = json_lead_byte(raw) else {
        return Err(err);
    };

    match jsonrepair::repair_to_value(raw, &jsonrepair::Options::default()) {
        Ok(v) if shape_matches_lead(lead, &v) => {
            tracing::warn!(
                raw_len = raw.len(),
                raw_prefix = %prefix(raw),
                "repaired malformed tool-call JSON"
            );
            Ok(v)
        }
        Ok(v) => {
            tracing::warn!(
                raw_len = raw.len(),
                raw_prefix = %prefix(raw),
                repaired_shape = shape_name(&v),
                "rejected JSON repair that changed the top-level shape; \
                 likely concatenated or truncated tool-call arguments"
            );
            Err(err)
        }
        Err(_) => Err(err),
    }
}

/// Conservative check: the first non-whitespace byte if it is a JSON
/// structural character (`{`, `[`, `"`), else `None`.
fn json_lead_byte(s: &str) -> Option<u8> {
    match s.trim_start().as_bytes().first() {
        Some(b @ (b'{' | b'[' | b'"')) => Some(*b),
        _ => None,
    }
}

fn shape_matches_lead(lead: u8, value: &serde_json::Value) -> bool {
    match lead {
        b'{' => value.is_object(),
        b'[' => value.is_array(),
        b'"' => value.is_string(),
        _ => false,
    }
}

fn shape_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

const PREFIX_CHARS: usize = 200;

fn prefix(raw: &str) -> &str {
    match raw.char_indices().nth(PREFIX_CHARS) {
        Some((idx, _)) => &raw[..idx],
        None => raw,
    }
}
