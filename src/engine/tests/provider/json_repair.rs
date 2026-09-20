use evotengine::provider::json_repair::try_repair_json;

#[test]
fn valid_json_passes_through() {
    let v = try_repair_json(r#"{"command":"ls"}"#).unwrap();
    assert_eq!(v["command"], "ls");
}

#[test]
fn trailing_comma_repaired() {
    let v = try_repair_json(r#"{"command": "ls",}"#).unwrap();
    assert_eq!(v["command"], "ls");
}

#[test]
fn truncated_object_repaired() {
    let v = try_repair_json(r#"{"command": "ls""#).unwrap();
    assert_eq!(v["command"], "ls");
}

#[test]
fn plain_text_not_repaired() {
    assert!(try_repair_json("hello world").is_err());
}

#[test]
fn empty_string_not_repaired() {
    assert!(try_repair_json("").is_err());
}

#[test]
fn markdown_fence_not_repaired() {
    assert!(try_repair_json("```json\n{}\n```").is_err());
}

#[test]
fn valid_array_passes_through() {
    let v = try_repair_json(r#"[1, 2, 3]"#).unwrap();
    assert_eq!(v, serde_json::json!([1, 2, 3]));
}

#[test]
fn valid_string_passes_through() {
    let v = try_repair_json(r#""hello""#).unwrap();
    assert_eq!(v, serde_json::json!("hello"));
}

/// Concatenated argument objects (what a stream produces when several
/// function calls' deltas land in one buffer) must not be "repaired" into an
/// array: the input starts with `{` so only an object is an acceptable
/// repair. The caller falls back to `{}` and schema validation reports the
/// real problem (missing required fields) instead of "do not wrap arguments
/// in an array".
#[test]
fn concatenated_objects_are_not_repaired_into_array() {
    let raw = r#"{"command":"a"}{"command":"b"}{"command":"c | jq '[.[""#;
    assert!(try_repair_json(raw).is_err());
}

/// A truncated single object still repairs to an object.
#[test]
fn truncated_object_with_nested_quotes_repaired_as_object() {
    let raw = r#"{"command":"c | jq '[.[""#;
    let v = try_repair_json(raw).unwrap();
    assert!(v.is_object());
    assert!(v["command"]
        .as_str()
        .is_some_and(|s| s.starts_with("c | jq")));
}

/// Repair that changes `[` input into a non-array is rejected too.
#[test]
fn array_lead_must_repair_to_array() {
    let v = try_repair_json(r#"[{"a":1},{"b":2"#).unwrap();
    assert!(v.is_array());
    assert_eq!(v.as_array().map(Vec::len), Some(2));
}
