//! BM25 inverted index over code chunks, with code-aware reranking.
//!
//! The index is organized around files as the source of truth: each indexed
//! file carries its `(mtime, size)` and its chunks. On refresh we re-`stat`
//! the tree, re-chunk only files whose `(mtime, size)` changed, drop files
//! that disappeared, and rebuild the inverted index from the surviving chunks.
//! Re-chunking is the expensive part (file I/O + tree-sitter); rebuilding the
//! inverted table is pure in-memory work, so we always rebuild it wholesale
//! rather than maintaining it incrementally (which is fiddly and error-prone).

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use ignore::WalkBuilder;
use rayon::prelude::*;
use tokio_util::sync::CancellationToken;

use super::chunk::chunk_file;
use super::chunk::split_identifier;
use super::chunk::tokenize;
use super::chunk::Chunk;
use super::rank;

/// BM25 tuning constants.
const K1: f32 = 1.2;
const B: f32 = 0.75;

/// A ranked search hit.
pub struct Hit<'a> {
    pub chunk: &'a Chunk,
    pub score: f32,
}

/// An indexed file: its change-detection stamp and its chunks.
#[derive(Clone)]
struct FileEntry {
    /// Modification time in nanoseconds since the Unix epoch (0 if unavailable).
    mtime: u64,
    /// File size in bytes.
    size: u64,
    /// Chunks parsed from this file. `Arc` so rebuilding the flattened chunk
    /// list only copies pointers, never chunk contents.
    chunks: Vec<Arc<Chunk>>,
}

/// A BM25 index built from a directory tree, refreshable in place.
pub struct Bm25Index {
    /// Source of truth: indexed files keyed by absolute path.
    files: HashMap<PathBuf, FileEntry>,
    /// Derived: all chunks flattened in deterministic (path-sorted) order.
    chunks: Vec<Arc<Chunk>>,
    /// Derived: term -> [(chunk_idx, term_frequency)].
    inverted: HashMap<String, Vec<(u32, f32)>>,
    /// Derived: mean chunk token length (>= 1).
    avg_doc_len: f32,
}

impl Bm25Index {
    /// Build a fresh index for `root` by refreshing an empty one.
    pub fn build(root: &Path, cancel: &CancellationToken) -> Self {
        Self::empty()
            .refreshed(root, cancel)
            .unwrap_or_else(Self::empty)
    }

    fn empty() -> Self {
        Self {
            files: HashMap::new(),
            chunks: Vec::new(),
            inverted: HashMap::new(),
            avg_doc_len: 1.0,
        }
    }

    /// Re-scan `root`, re-chunk only new/modified files, drop removed files,
    /// and rebuild the index. Returns `Some(new_index)` if anything changed,
    /// or `None` if the tree is unchanged (caller keeps the existing index).
    /// Honors cancellation between phases.
    pub fn refreshed(&self, root: &Path, cancel: &CancellationToken) -> Option<Self> {
        let current = collect_files(root);
        if cancel.is_cancelled() {
            return None;
        }

        // Partition the current tree against the cached file set.
        let mut present: HashSet<PathBuf> = HashSet::with_capacity(current.len());
        let mut changed: Vec<PathBuf> = Vec::new();
        for (path, mtime, size) in &current {
            present.insert(path.clone());
            match self.files.get(path) {
                Some(e) if e.mtime == *mtime && e.size == *size => {}
                _ => changed.push(path.clone()),
            }
        }
        let removed = self.files.keys().any(|p| !present.contains(p));

        if changed.is_empty() && !removed {
            return None; // tree unchanged — nothing to rebuild
        }

        // Re-chunk only the changed files, in parallel. Capture (mtime, size)
        // alongside the read so the stamp matches the bytes we chunked.
        let rechunked: Vec<(PathBuf, FileEntry)> = changed
            .par_iter()
            .map(|path| {
                let (mtime, size) = file_meta(path);
                let chunks = chunk_file(path).into_iter().map(Arc::new).collect();
                (path.clone(), FileEntry {
                    mtime,
                    size,
                    chunks,
                })
            })
            .collect();

        if cancel.is_cancelled() {
            return None;
        }

        // Carry over unchanged files (cheap Arc clones), drop removed ones by
        // simply not copying them, then upsert the re-chunked entries.
        let mut files: HashMap<PathBuf, FileEntry> = HashMap::with_capacity(current.len());
        for path in &present {
            if let Some(entry) = self.files.get(path) {
                files.insert(path.clone(), entry.clone());
            }
        }
        for (path, entry) in rechunked {
            files.insert(path, entry);
        }

        Some(Self::from_files(files))
    }

    /// Rebuild the derived chunk list and inverted index from `files`.
    fn from_files(files: HashMap<PathBuf, FileEntry>) -> Self {
        // Flatten in path-sorted order so chunk indices — and therefore tie
        // breaking in ranking — are deterministic across rebuilds.
        let mut paths: Vec<&PathBuf> = files.keys().collect();
        paths.sort();
        let mut chunks: Vec<Arc<Chunk>> = Vec::new();
        for p in paths {
            for c in &files[p].chunks {
                chunks.push(Arc::clone(c));
            }
        }

        let num = chunks.len();
        let avg_doc_len = if num == 0 {
            1.0
        } else {
            chunks.iter().map(|c| c.tokens.len()).sum::<usize>() as f32 / num as f32
        };

        let mut inverted: HashMap<String, Vec<(u32, f32)>> = HashMap::new();
        for (idx, chunk) in chunks.iter().enumerate() {
            let mut tf: HashMap<&str, u32> = HashMap::new();
            for t in &chunk.tokens {
                *tf.entry(t.as_str()).or_default() += 1;
            }
            for (term, count) in tf {
                inverted
                    .entry(term.to_string())
                    .or_default()
                    .push((idx as u32, count as f32));
            }
        }

        Self {
            files,
            chunks,
            inverted,
            avg_doc_len,
        }
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Search for `query`, returning up to `top_k` reranked hits.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<Hit<'_>> {
        if self.chunks.is_empty() {
            return Vec::new();
        }
        let query_terms = tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }
        let query_stems: Vec<String> = query_terms
            .iter()
            .flat_map(|t| split_identifier(t))
            .collect();

        // Phase 1: BM25 base scores.
        let n = self.chunks.len() as f32;
        let mut scores = vec![0.0f32; self.chunks.len()];
        for term in &query_terms {
            let Some(postings) = self.inverted.get(term) else {
                continue;
            };
            let df = postings.len() as f32;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for &(idx, tf) in postings {
                let chunk = &self.chunks[idx as usize];
                let doc_len = chunk.tokens.len() as f32;
                let norm = 1.0 - B + B * (doc_len / self.avg_doc_len);
                scores[idx as usize] += idf * (tf * (K1 + 1.0)) / (tf + K1 * norm);
            }
        }

        // Phase 2: rerank top BM25 candidates with code-aware signals.
        let mut candidates: Vec<(usize, f32)> = scores
            .iter()
            .enumerate()
            .filter(|(_, s)| **s > 0.0)
            .map(|(i, s)| (i, *s))
            .collect();
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
        candidates.truncate(50);

        for (idx, score) in &mut candidates {
            let chunk = &self.chunks[*idx];
            *score += rank::stem_boost(&query_stems, chunk)
                + rank::definition_boost(&query_stems, chunk)
                + rank::noise_penalty(&chunk.file_path);
        }
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1));

        // Phase 3: file saturation — at most 2 chunks per file in the output.
        let mut per_file: HashMap<&str, usize> = HashMap::new();
        let mut hits = Vec::with_capacity(top_k);
        for (idx, score) in candidates {
            let chunk = self.chunks[idx].as_ref();
            let count = per_file.entry(chunk.file_path.as_str()).or_default();
            if *count >= 2 {
                continue;
            }
            *count += 1;
            hits.push(Hit { chunk, score });
            if hits.len() >= top_k {
                break;
            }
        }
        hits
    }
}

/// Source file extensions worth indexing.
const SOURCE_EXTS: &[&str] = &[
    "rs", "py", "pyi", "js", "jsx", "mjs", "cjs", "ts", "tsx", "go", "java", "c", "h", "cc", "cpp",
    "cxx", "hpp", "hh", "rb", "kt", "scala", "swift", "php", "cs", "lua", "ex", "exs", "hs",
];

/// Read a file's change-detection stamp: `(mtime_nanos, size_bytes)`.
/// Missing/unreadable metadata yields `(0, 0)`, which simply forces a re-chunk.
fn file_meta(path: &Path) -> (u64, u64) {
    match std::fs::metadata(path) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            (mtime, m.len())
        }
        Err(_) => (0, 0),
    }
}

/// Gitignore-aware walk collecting source files with their change stamps.
fn collect_files(root: &Path) -> Vec<(PathBuf, u64, u64)> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(root).require_git(false).build();
    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let is_source = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| SOURCE_EXTS.contains(&ext))
            .unwrap_or(false);
        if !is_source {
            continue;
        }
        let path = entry.into_path();
        let (mtime, size) = file_meta(&path);
        files.push((path, mtime, size));
    }
    files
}
