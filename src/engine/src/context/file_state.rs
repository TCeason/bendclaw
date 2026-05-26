//! File read state tracking for dedup and post-compaction restore.
//!
//! Tracks which files have been read, their modification times, and recency.
//! Consumed by:
//! - `tool_exec` (dedup: skip re-reads of unchanged files)
//! - `file_restore` (restore: re-inject recently read files after compaction)

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

/// A record of a single file read.
#[derive(Debug, Clone)]
pub struct FileReadEntry {
    /// Absolute path as seen by the tool.
    pub path: String,
    /// File modification time (ms since epoch) at the time of read.
    pub mtime_ms: u64,
    /// Total line count of the file.
    pub total_lines: usize,
    /// Timestamp (ms since epoch) when this read occurred.
    pub read_at: u64,
}

/// Tracks recently read files for dedup and restore purposes.
#[derive(Debug, Default)]
pub struct FileReadState {
    entries: HashMap<String, FileReadEntry>,
}

impl FileReadState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful file read.
    pub fn record(&mut self, path: &str, mtime_ms: u64, total_lines: usize) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.entries.insert(path.to_string(), FileReadEntry {
            path: path.to_string(),
            mtime_ms,
            total_lines,
            read_at: now,
        });
    }

    /// Check if a file is unchanged since last read.
    ///
    /// Returns `Some(entry)` if the path was previously read AND the current
    /// mtime matches the recorded mtime (file hasn't been modified).
    pub fn is_unchanged(&self, path: &str, current_mtime_ms: u64) -> Option<&FileReadEntry> {
        self.entries
            .get(path)
            .filter(|entry| entry.mtime_ms == current_mtime_ms)
    }

    /// Get the N most recently read files, sorted by recency (newest first).
    pub fn recent_files(&self, max: usize) -> Vec<&FileReadEntry> {
        let mut entries: Vec<&FileReadEntry> = self.entries.values().collect();
        entries.sort_by(|a, b| b.read_at.cmp(&a.read_at));
        entries.truncate(max);
        entries
    }

    /// Invalidate a path (called after Edit/Write modifies it).
    pub fn invalidate(&mut self, path: &str) {
        self.entries.remove(path);
    }

    /// Set the `read_at` timestamp for a path (test helper).
    pub fn set_read_at(&mut self, path: &str, ts: u64) {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.read_at = ts;
        }
    }
}

/// Thread-safe shared handle to `FileReadState`.
pub type SharedFileReadState = Arc<Mutex<FileReadState>>;

/// Create a new shared file read state.
pub fn new_shared() -> SharedFileReadState {
    Arc::new(Mutex::new(FileReadState::new()))
}
