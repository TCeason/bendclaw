//! File read state tracking for dedup and post-compaction restore.
//!
//! Tracks which files have been read, their modification times, and recency.
//! Consumed by:
//! - `tool_exec` (dedup: skip re-reads of unchanged files)
//! - `file_restore` (restore: re-inject recently read files after compaction)

use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use tokio::sync::Mutex;

const DEFAULT_CAPACITY: usize = 128;

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
/// Uses LRU eviction to bound memory usage.
#[derive(Debug)]
pub struct FileReadState {
    cache: LruCache<String, FileReadEntry>,
}

impl FileReadState {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(cap).unwrap_or(NonZeroUsize::new(1).unwrap())),
        }
    }

    /// Record a successful file read. Promotes the entry to most-recent.
    pub fn record(&mut self, path: &str, mtime_ms: u64, total_lines: usize) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.cache.put(path.to_string(), FileReadEntry {
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
    /// Promotes the entry in the LRU on hit.
    pub fn is_unchanged(&mut self, path: &str, current_mtime_ms: u64) -> Option<&FileReadEntry> {
        let entry = self.cache.get(path)?;
        if entry.mtime_ms == current_mtime_ms {
            Some(entry)
        } else {
            None
        }
    }

    /// Get the N most recently read files (sorted by recency, newest first).
    pub fn recent_files(&self, max: usize) -> Vec<&FileReadEntry> {
        let mut entries: Vec<&FileReadEntry> = self.cache.iter().map(|(_, entry)| entry).collect();
        entries.sort_by(|a, b| b.read_at.cmp(&a.read_at));
        entries.truncate(max);
        entries
    }

    /// Invalidate a path (called after Edit/Write modifies it).
    pub fn invalidate(&mut self, path: &str) {
        self.cache.pop(path);
    }

    /// Set the `read_at` timestamp for a path (test helper).
    pub fn set_read_at(&mut self, path: &str, ts: u64) {
        if let Some(entry) = self.cache.get_mut(path) {
            entry.read_at = ts;
        }
    }
}

impl Default for FileReadState {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe shared handle to `FileReadState`.
pub type SharedFileReadState = Arc<Mutex<FileReadState>>;

/// Create a new shared file read state.
pub fn new_shared() -> SharedFileReadState {
    Arc::new(Mutex::new(FileReadState::new()))
}
