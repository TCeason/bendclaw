use serde_json::json;

pub fn schema() -> serde_json::Value {
    let year = chrono::Utc::now().format("%Y");
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": format!("The search query. Be specific and use keywords for better results. For example, use 'Rust async runtime tokio {year}' instead of 'tell me about async in Rust'.")
            },
            "count": {
                "type": "integer",
                "description": "Number of results to return (default 5, max 10)"
            }
        },
        "required": ["query"]
    })
}
