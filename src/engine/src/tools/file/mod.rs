//! File tools — read and write files with safety limits.

mod read;
mod read_slim;
mod write;

use std::path::Path;

pub use read::ReadFileTool;
pub use read_slim::ReadSlimFileTool;
pub use write::WriteFileTool;

/// 20 MB limit for image files
pub(crate) const MAX_IMAGE_SIZE_BYTES: u64 = 20 * 1024 * 1024;

pub(crate) fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp")
    )
}

pub(crate) fn get_image_mime_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        _ => None,
    }
}
