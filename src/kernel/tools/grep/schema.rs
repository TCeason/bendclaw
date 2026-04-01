use serde_json::json;

pub fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Regular expression pattern to search for."
            },
            "path": {
                "type": "string",
                "description": "Absolute or relative path to search in. Defaults to the workspace directory."
            },
            "file_pattern": {
                "type": "string",
                "description": "Optional glob to filter files (e.g. '*.rs', '*.py')."
            }
        },
        "required": ["pattern"]
    })
}
