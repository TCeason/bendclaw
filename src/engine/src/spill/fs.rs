use std::path::PathBuf;

use super::types::SpillError;
use super::types::SpillRef;
use super::types::SpillRequest;

/// Local filesystem spill: writes large text to a file, returns a reference.
pub struct FsSpill {
    dir: PathBuf,
    threshold_bytes: usize,
    preview_bytes: usize,
}

impl FsSpill {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            threshold_bytes: crate::tools::validation::MAX_TOOL_RESULT_BYTES,
            preview_bytes: 4_000,
        }
    }

    pub fn with_threshold_bytes(mut self, value: usize) -> Self {
        self.threshold_bytes = value;
        self
    }

    pub fn with_preview_bytes(mut self, value: usize) -> Self {
        self.preview_bytes = value;
        self
    }

    pub async fn spill(&self, req: SpillRequest) -> Result<Option<SpillRef>, SpillError> {
        if req.text.len() <= self.threshold_bytes {
            return Ok(None);
        }

        let safe_key = sanitize_key(&req.key);
        let path = self.dir.join(format!("{safe_key}.txt"));

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, &req.text).await?;

        let preview = make_preview(&req.text, self.preview_bytes);

        Ok(Some(SpillRef {
            size_bytes: req.text.len(),
            path,
            preview,
        }))
    }
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn make_preview(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let boundary = text.floor_char_boundary(max_bytes);
    let cut = text[..boundary].rfind('\n').unwrap_or(boundary);
    text[..cut].to_string()
}
