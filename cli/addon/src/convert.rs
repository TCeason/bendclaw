use serde::Deserialize;

/// Content block from JS — typed deserialization for queryWithContent.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum JsContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

/// Convert a JSON string of content blocks into engine Content items.
pub(crate) fn parse_content_blocks(
    json: &str,
) -> std::result::Result<Vec<evot_engine::Content>, String> {
    let blocks: Vec<JsContent> =
        serde_json::from_str(json).map_err(|e| format!("parse content: {e}"))?;

    let input: Vec<evot_engine::Content> = blocks
        .into_iter()
        .filter_map(|block| match block {
            JsContent::Text { text } if !text.is_empty() => {
                Some(evot_engine::Content::Text { text })
            }
            JsContent::Image { data, mime_type } if !data.is_empty() => {
                Some(evot_engine::Content::Image { data, mime_type })
            }
            _ => None,
        })
        .collect();

    Ok(input)
}
