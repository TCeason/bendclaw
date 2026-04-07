//! Web fetch tool — fetch content from a URL with timeout and size limits.

use std::io::Cursor;

use async_trait::async_trait;

use crate::types::*;

const MAX_RESPONSE_SIZE: usize = 512 * 1024; // 512KB
const TIMEOUT_SECS: u64 = 30;

/// Fetch content from a URL. Returns the response body as text.
pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentTool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn label(&self) -> &str {
        "Fetch URL"
    }

    fn description(&self) -> &str {
        "Fetches content from a URL. Returns the response body as text with a 512KB size limit.\n\
         \n\
         Use this tool to retrieve web pages, API responses, or any HTTP-accessible content.\n\
         Supports custom HTTP headers for authenticated requests.\n\
         \n\
         Parameters:\n\
         - url (required): The URL to fetch\n\
         - headers (optional): A JSON object of HTTP headers to include in the request"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'url' parameter".into()))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .build()
            .map_err(|e| ToolError::Failed(format!("Failed to create HTTP client: {e}")))?;

        let mut req = client.get(url);

        if let Some(headers) = params.get("headers").and_then(|h| h.as_object()) {
            for (key, value) in headers {
                if let Some(val) = value.as_str() {
                    req = req.header(key, val);
                }
            }
        }

        let cancel = ctx.cancel;

        let response = tokio::select! {
            _ = cancel.cancelled() => {
                return Err(ToolError::Cancelled);
            }
            result = req.send() => {
                result.map_err(|e| ToolError::Failed(format!("Fetch failed: {e}")))?
            }
        };

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if status >= 400 {
            let body = response
                .text()
                .await
                .map_err(|e| ToolError::Failed(format!("Failed to read response body: {e}")))?;
            return Ok(ToolResult {
                content: vec![Content::Text {
                    text: format!("HTTP {status} error fetching {url}"),
                }],
                details: serde_json::json!({ "status": status, "error": true, "body": body }),
            });
        }

        let body = response
            .text()
            .await
            .map_err(|e| ToolError::Failed(format!("Failed to read response body: {e}")))?;

        let output = if content_type.contains("text/html") {
            html_to_markdown(&body, url).unwrap_or(body)
        } else {
            body
        };

        let mut text = output;
        if text.len() > MAX_RESPONSE_SIZE {
            text.truncate(MAX_RESPONSE_SIZE);
            text.push_str("\n... (response truncated at 512KB)");
        }

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::json!({ "status": status }),
        })
    }
}

/// Extract readable content from HTML and convert it to markdown.
///
/// Returns `None` if the input cannot be parsed or has no extractable content,
/// allowing the caller to fall back to the raw text.
fn html_to_markdown(html: &str, url: &str) -> Option<String> {
    let mut cursor = Cursor::new(html.as_bytes());
    let parsed_url = reqwest::Url::parse(url).ok()?;
    let article = readability::extractor::extract(&mut cursor, &parsed_url).ok()?;

    let md = htmd::convert(&article.content).ok()?;
    let trimmed = md.trim();
    if trimmed.is_empty() {
        return None;
    }

    let title = article.title.trim();
    if title.is_empty() {
        Some(trimmed.to_string())
    } else {
        Some(format!("# {title}\n\n{trimmed}"))
    }
}
