//! Image resize — cap image dimensions before sending to LLM.
//!
//! Anthropic charges `width * height / 750` tokens per image.
//! Capping at 2000×2000 ensures images never exceed ~5333 tokens.
//!
//! Strategy (mirrors claudecode):
//!   1. Images within limits pass through unchanged
//!   2. Oversized images get proportionally scaled to fit 2000×2000
//!   3. Result re-encoded as JPEG quality 85 to keep size reasonable

use base64::Engine;

/// Maximum dimensions (matching claudecode's IMAGE_MAX_WIDTH/HEIGHT).
const MAX_DIM: u32 = 2000;

/// Result: (base64_data, mime_type)
pub fn resize_image(data: &str, mime_type: &str) -> Result<(String, String), String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("base64 decode: {e}"))?;

    let img = image::load_from_memory(&decoded).map_err(|e| format!("decode image: {e}"))?;

    let (w, h) = (img.width(), img.height());
    if w <= MAX_DIM && h <= MAX_DIM {
        return Ok((data.to_string(), mime_type.to_string()));
    }

    // Scale proportionally to fit within MAX_DIM
    let ratio = MAX_DIM as f64 / w.max(h) as f64;
    let new_w = (w as f64 * ratio) as u32;
    let new_h = (h as f64 * ratio) as u32;

    let resized = img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3);

    let mut buf = std::io::Cursor::new(Vec::new());
    resized
        .write_to(&mut buf, image::ImageFormat::Jpeg)
        .map_err(|e| format!("encode jpeg: {e}"))?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
    Ok((encoded, "image/jpeg".to_string()))
}
