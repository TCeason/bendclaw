//! Per-file mutation queue: serializes write operations targeting the same file
//! while allowing operations on different files to run in parallel.
//!
//! Mirrors pi's `withFileMutationQueue` pattern, including eviction of unused
//! entries on guard release.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as SyncMutex;

use tokio::sync::Mutex;
use tokio::sync::OwnedMutexGuard;

/// Global per-file mutex registry.
///
/// Each unique canonical path gets its own mutex. Operations on different files
/// proceed concurrently; operations on the same file are serialized.
/// Entries are evicted when the last guard is dropped.
static FILE_MUTEXES: std::sync::LazyLock<SyncMutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    std::sync::LazyLock::new(|| SyncMutex::new(HashMap::new()));

/// Resolve a file path to a canonical key for the mutex map.
/// Uses `tokio::fs::canonicalize` when the file exists; falls back to the
/// absolute path for files that don't exist yet (e.g. new file creation).
async fn resolve_mutex_key(path: &Path) -> PathBuf {
    match tokio::fs::canonicalize(path).await {
        Ok(canonical) => canonical,
        Err(_) => path.to_path_buf(),
    }
}

/// RAII guard for a per-file lock. Releases the async lock on drop, then
/// evicts the registry entry if no other task is waiting on it.
pub struct FileLockGuard {
    _inner: OwnedMutexGuard<()>,
    key: PathBuf,
}

impl Drop for FileLockGuard {
    fn drop(&mut self) {
        let map = match FILE_MUTEXES.lock() {
            Ok(m) => m,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Evict if we hold the only remaining reference (registry + this Arc).
        // After _inner is dropped (field order: _inner before key, but Drop runs
        // before fields are dropped), the OwnedMutexGuard is still alive here.
        // The registry holds one Arc, we hold one via _inner. If strong_count == 1
        // after we conceptually release, it means no one else is waiting.
        // Since _inner hasn't been dropped yet at this point, strong_count == 2
        // means "only us + registry", so evict.
        if let Some(arc) = map.get(&self.key) {
            if Arc::strong_count(arc) <= 2 {
                drop(map);
                let mut map = match FILE_MUTEXES.lock() {
                    Ok(m) => m,
                    Err(poisoned) => poisoned.into_inner(),
                };
                // Re-check after re-acquiring — another task may have inserted.
                if let Some(arc) = map.get(&self.key) {
                    if Arc::strong_count(arc) <= 2 {
                        map.remove(&self.key);
                    }
                }
            }
        }
    }
}

/// Acquire the per-file lock for the given path.
/// Returns a guard that releases the lock and evicts the registry entry when dropped.
pub async fn acquire_file_lock(path: &Path) -> FileLockGuard {
    let key = resolve_mutex_key(path).await;
    let mutex = {
        let mut map = match FILE_MUTEXES.lock() {
            Ok(m) => m,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let guard = mutex.lock_owned().await;
    FileLockGuard { _inner: guard, key }
}
