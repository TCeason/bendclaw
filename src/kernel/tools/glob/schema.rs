use serde_json::json;

pub fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Glob pattern to match file names, e.g. '*.rs', '*.test.ts', 'Cargo.toml'."
            },
            "path": {
                "type": "string",
                "description": "Absolute or relative path to search in. Defaults to the workspace directory."
            }
        },
        "required": ["pattern"]
    })
}
