use std::path::PathBuf;

/// Spill request: a key (used as filename) and the text to spill.
pub struct SpillRequest {
    pub key: String,
    pub text: String,
}

/// Reference to a spilled file.
pub struct SpillRef {
    pub path: PathBuf,
    pub size_bytes: usize,
    pub preview: String,
}

/// Spill error.
#[derive(Debug)]
pub enum SpillError {
    Io(std::io::Error),
}

impl std::fmt::Display for SpillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "spill io: {e}"),
        }
    }
}

impl std::error::Error for SpillError {}

impl From<std::io::Error> for SpillError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
