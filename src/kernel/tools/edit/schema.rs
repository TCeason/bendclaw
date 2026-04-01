use serde_json::json;

pub fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file within the workspace"
            },
            "old_string": {
                "type": "string",
                "description": "The exact string to search for in the file"
            },
            "new_string": {
                "type": "string",
                "description": "The replacement string"
            }
        },
        "required": ["path", "old_string", "new_string"]
    })
}
