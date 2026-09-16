//! Protocol error fields independent of any gateway or vendor topology.

pub(super) fn error_node(value: &serde_json::Value) -> &serde_json::Value {
    value
        .pointer("/response/error")
        .or_else(|| value.get("error"))
        .unwrap_or(value)
}

pub(super) fn model_not_found(message: &str, value: Option<&serde_json::Value>) -> bool {
    if value
        .map(error_node)
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str)
        == Some("model_not_found")
    {
        return true;
    }
    let lower = message.to_lowercase();
    lower.contains("model_not_found")
        || lower.contains("model not found")
        || (lower.contains("model") && lower.contains("does not exist"))
}

pub(super) fn permanent_type(value: &serde_json::Value) -> bool {
    let error = error_node(value);
    ["type", "code"].iter().any(|field| {
        matches!(
            error.get(field).and_then(serde_json::Value::as_str),
            Some(
                "invalid_request_error"
                    | "not_found_error"
                    | "model_not_found"
                    | "unsupported_model"
                    | "model_not_supported"
                    | "unsupported_operation"
            )
        )
    })
}
