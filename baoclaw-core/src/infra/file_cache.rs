/// FileCache — LRU file cache with change detection.
///
/// Tracks files that have been read during a session. On subsequent reads:
/// - **Hit** (unchanged): returns a stub instead of the full content, saving tokens.
/// - **Changed**: records the new version so the model sees fresh content.
/// - **Miss**: records the file for future lookups.
///
/// Only metadata (path, content hash, mtime, line count) is stored — no file
/// contents — to keep memory usage minimal.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

/// Metadata for a cached file.
#[derive(Debug)]
pub struct FileCacheEntry {
    pub path: PathBuf,
    /// FNV-1a hash of file content for fast change detection.
    pub content_hash: u64,
    /// File modification time at last read.
    pub mtime: SystemTime,
    /// Instant of last access (for LRU eviction).
    pub last_accessed: Instant,
    /// Number of lines in the file (included in stub messages).
    pub line_count: usize,
}

/// Result of checking a file against the cache.
#[derive(Debug, PartialEq)]
pub enum CacheStatus {
    /// File was read before and has not changed.
    Hit,
    /// File was read before but has been modified externally.
    Changed,
    /// File has never been read (or was evicted from the cache).
    Miss,
}

/// LRU file cache. Stores metadata only (no file contents).
pub struct FileCache {
    entries: HashMap<PathBuf, FileCacheEntry>,
    max_entries: usize,
}

impl FileCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
        }
    }

    /// Create with default capacity (100 entries).
    pub fn default_capacity() -> Self {
        Self::new(100)
    }

    /// Check whether `path` is in the cache and whether it has changed.
    ///
    /// Returns `Hit` if cached and unchanged, `Changed` if the file has been
    /// modified since last read, or `Miss` if not in the cache.
    pub fn check(&self, path: &Path) -> CacheStatus {
        let canonical = match std::fs::canonicalize(path) {
            Ok(c) => c,
            Err(_) => return CacheStatus::Miss,
        };

        match self.entries.get(&canonical) {
            Some(entry) => {
                // Read current content and compare hash for definitive change detection.
                // (mtime comparison alone is unreliable for same-second modifications.)
                match std::fs::read_to_string(&canonical) {
                    Ok(content) => {
                        let hash = fnv1a_hash(&content);
                        if hash == entry.content_hash {
                            CacheStatus::Hit
                        } else {
                            CacheStatus::Changed
                        }
                    }
                    Err(_) => CacheStatus::Changed,
                }
            }
            None => CacheStatus::Miss,
        }
    }

    /// Record that `path` has been read with `content`.
    ///
    /// If the cache is full, the least-recently-accessed entry is evicted.
    pub fn record(&mut self, path: &Path, content: &str) {
        let canonical = match std::fs::canonicalize(path) {
            Ok(c) => c,
            Err(_) => path.to_path_buf(),
        };

        let mtime = std::fs::metadata(&canonical)
            .and_then(|m| m.modified())
            .unwrap_or_else(|_| SystemTime::now());

        let line_count = content.lines().count();

        // Evict LRU entry if at capacity
        if self.entries.len() >= self.max_entries && !self.entries.contains_key(&canonical) {
            if let Some(lru_key) = self.lru_key() {
                self.entries.remove(&lru_key);
            }
        }

        self.entries.insert(
            canonical.clone(),
            FileCacheEntry {
                path: canonical,
                content_hash: fnv1a_hash(content),
                mtime,
                last_accessed: Instant::now(),
                line_count,
            },
        );
    }

    /// Build a stub message for a cache hit.
    ///
    /// The stub tells the model that the previously-read content is still valid,
    /// so it doesn't need the full file injected again.
    pub fn build_stub(&self, path: &Path) -> Option<String> {
        let canonical = match std::fs::canonicalize(path) {
            Ok(c) => c,
            Err(_) => path.to_path_buf(),
        };

        self.entries.get(&canonical).map(|entry| {
            format!(
                "[File cache hit: {} — content unchanged ({} lines). \
                 Previously read content is still valid.]",
                entry.path.display(),
                entry.line_count,
            )
        })
    }

    /// Touch the last-accessed time for a cached entry.
    pub fn touch(&mut self, path: &Path) {
        let canonical = match std::fs::canonicalize(path) {
            Ok(c) => c,
            Err(_) => path.to_path_buf(),
        };
        if let Some(entry) = self.entries.get_mut(&canonical) {
            entry.last_accessed = Instant::now();
        }
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Find the least-recently-accessed key for LRU eviction.
    fn lru_key(&self) -> Option<PathBuf> {
        self.entries
            .iter()
            .min_by_key(|(_, e)| e.last_accessed)
            .map(|(k, _)| k.clone())
    }
}

/// FNV-1a 64-bit hash — fast, good distribution, no external dependency.
fn fnv1a_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_miss_for_unknown_file() {
        let dir = TempDir::new().unwrap();
        let cache = FileCache::new(10);
        let path = dir.path().join("new.txt");
        fs::write(&path, "hello").unwrap();

        assert_eq!(cache.check(&path), CacheStatus::Miss);
    }

    #[test]
    fn test_hit_after_record() {
        let dir = TempDir::new().unwrap();
        let mut cache = FileCache::new(10);
        let path = dir.path().join("test.txt");
        fs::write(&path, "line 1\nline 2").unwrap();

        cache.record(&path, "line 1\nline 2");
        assert_eq!(cache.check(&path), CacheStatus::Hit);
    }

    #[test]
    fn test_changed_after_modification() {
        let dir = TempDir::new().unwrap();
        let mut cache = FileCache::new(10);
        let path = dir.path().join("test.txt");
        fs::write(&path, "original").unwrap();

        cache.record(&path, "original");
        // Modify file (need to change mtime — write with small delay)
        fs::write(&path, "modified content").unwrap();

        assert_eq!(cache.check(&path), CacheStatus::Changed);
    }

    #[test]
    fn test_lru_eviction() {
        let dir = TempDir::new().unwrap();
        let mut cache = FileCache::new(2);

        let f1 = dir.path().join("a.txt");
        let f2 = dir.path().join("b.txt");
        let f3 = dir.path().join("c.txt");

        fs::write(&f1, "aaa").unwrap();
        fs::write(&f2, "bbb").unwrap();
        fs::write(&f3, "ccc").unwrap();

        cache.record(&f1, "aaa");
        cache.record(&f2, "bbb");
        // Cache is now full (2 entries). Adding a third should evict f1.
        cache.record(&f3, "ccc");

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.check(&f1), CacheStatus::Miss); // evicted
        assert_eq!(cache.check(&f2), CacheStatus::Hit);  // still present
        assert_eq!(cache.check(&f3), CacheStatus::Hit);  // just added
    }

    #[test]
    fn test_build_stub() {
        let dir = TempDir::new().unwrap();
        let mut cache = FileCache::new(10);
        let path = dir.path().join("test.txt");
        fs::write(&path, "line 1\nline 2\nline 3").unwrap();

        cache.record(&path, "line 1\nline 2\nline 3");
        let stub = cache.build_stub(&path).unwrap();

        assert!(stub.contains("cache hit"));
        assert!(stub.contains("3 lines"));
    }

    #[test]
    fn test_build_stub_not_cached() {
        let dir = TempDir::new().unwrap();
        let cache = FileCache::new(10);
        let path = dir.path().join("test.txt");
        fs::write(&path, "hello").unwrap();

        assert!(cache.build_stub(&path).is_none());
    }

    #[test]
    fn test_clear() {
        let dir = TempDir::new().unwrap();
        let mut cache = FileCache::new(10);
        let path = dir.path().join("test.txt");
        fs::write(&path, "hello").unwrap();

        cache.record(&path, "hello");
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert_eq!(cache.len(), 0);
    }
}
