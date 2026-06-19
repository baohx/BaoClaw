//! Memory store implementation.
//!
//! Persistent memory store backed by a JSONL file.
//! Supports both global (~/.baoclaw/) and project-level (<cwd>/.baoclaw/) memory.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::Mutex;

use crate::engine::memory::{DecayConfig, apply_decay};
use crate::engine::memory::archive::{MemoryArchive, ArchiveResult};

const MEMORY_FILE: &str = "memory.jsonl";

/// Errors that can occur during memory store operations.
#[derive(Debug)]
pub enum MemoryError {
    /// IO error (file read/write failure, permission denied, etc.)
    Io(std::io::Error),
    /// Serialization or deserialization error
    Serde(serde_json::Error),
    /// A corrupted entry was encountered during read.
    /// The entry is skipped but the error is surfaced for logging.
    Corrupted { line: usize, reason: String },
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Serde(e) => write!(f, "Serialization error: {}", e),
            Self::Corrupted { line, reason } => {
                write!(f, "Corrupted entry at line {}: {}", line, reason)
            }
        }
    }
}

impl std::error::Error for MemoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Serde(e) => Some(e),
            Self::Corrupted { .. } => None,
        }
    }
}

impl From<std::io::Error> for MemoryError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for MemoryError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MemoryCategory {
    #[serde(rename = "fact")]
    Fact,
    #[serde(rename = "preference")]
    Preference,
    #[serde(rename = "decision")]
    Decision,
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fact => write!(f, "fact"),
            Self::Preference => write!(f, "preference"),
            Self::Decision => write!(f, "decision"),
        }
    }
}

/// Default importance for new memories
const DEFAULT_IMPORTANCE: f64 = 0.5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub category: MemoryCategory,
    pub created_at: String,
    pub source: String,
    /// Importance score (0.0-1.0), used for memory decay.
    /// Higher values indicate more important memories that should be retained longer.
    /// Default: 0.5
    #[serde(default = "default_importance")]
    pub importance: f64,
    /// Number of times this memory has been recalled.
    /// Increases each time the memory is accessed/referenced.
    #[serde(default)]
    pub recall_count: u32,
    /// ISO8601 timestamp of the last time this memory was recalled.
    /// Updated when the memory is referenced in a response.
    #[serde(default)]
    pub last_recalled_at: Option<String>,
    /// Whether this memory has been archived.
    /// Archived memories are moved to a separate storage file.
    #[serde(default)]
    pub archived: bool,
}

fn default_importance() -> f64 {
    DEFAULT_IMPORTANCE
}

/// Persistent memory store backed by a JSONL file.
/// Supports both global (~/.baoclaw/) and project-level (<cwd>/.baoclaw/) memory.
pub struct MemoryStore {
    entries: Mutex<Vec<MemoryEntry>>,
    file_path: Mutex<PathBuf>,
}

impl MemoryStore {
    /// Load global memories from ~/.baoclaw/memory.jsonl.
    pub fn load() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let file_path = PathBuf::from(&home).join(".baoclaw").join(MEMORY_FILE);
        let entries = Self::read_file(&file_path);
        eprintln!("Loaded {} long-term memories from {}", entries.len(), file_path.display());
        Self {
            entries: Mutex::new(entries),
            file_path: Mutex::new(file_path),
        }
    }

    /// Load project-level memories from <cwd>/.baoclaw/memory.jsonl.
    /// Falls back to global if project dir doesn't have .baoclaw/.
    pub fn load_for_project(cwd: &std::path::Path) -> Self {
        let project_path = cwd.join(".baoclaw").join(MEMORY_FILE);
        let file_path = if cwd.join(".baoclaw").exists() {
            project_path
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(&home).join(".baoclaw").join(MEMORY_FILE)
        };
        let entries = Self::read_file(&file_path);
        eprintln!("Loaded {} project memories from {}", entries.len(), file_path.display());
        Self {
            entries: Mutex::new(entries),
            file_path: Mutex::new(file_path),
        }
    }

    /// Switch to a different project's memory store.
    pub async fn switch_project(&self, cwd: &std::path::Path) {
        let new_path = if cwd.join(".baoclaw").exists() {
            cwd.join(".baoclaw").join(MEMORY_FILE)
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(&home).join(".baoclaw").join(MEMORY_FILE)
        };
        let new_entries = Self::read_file(&new_path);
        eprintln!("Switched memory to {} ({} entries)", new_path.display(), new_entries.len());
        *self.entries.lock().await = new_entries;
        *self.file_path.lock().await = new_path;
    }

    fn read_file(path: &PathBuf) -> Vec<MemoryEntry> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(), // File doesn't exist yet → empty Vec (not an error)
        };
        let mut entries = Vec::new();
        for (line_no, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<MemoryEntry>(line) {
                Ok(e) => entries.push(e),
                Err(e) => {
                    // Log corrupted line but don't abort reading (degraded mode: skip + warn)
                    eprintln!(
                        "WARNING: corrupted memory entry at line {} in {}: {}",
                        line_no,
                        path.display(),
                        e
                    );
                }
            }
        }
        entries
    }

    fn write_all_sync(path: &PathBuf, entries: &[MemoryEntry]) -> Result<(), MemoryError> {
        let lines: Vec<String> = entries
            .iter()
            .map(|e| serde_json::to_string(e))
            .collect::<Result<Vec<_>, _>>()?;
        std::fs::write(path, lines.join("\n") + "\n")?;
        Ok(())
    }

    /// Add a new memory entry.
    ///
    /// Returns the created entry on success, or `MemoryError` on write failure.
    /// The entry is always added to in-memory state; the error reflects a
    /// filesystem persistence failure.
    pub async fn add(
        &self,
        content: String,
        category: MemoryCategory,
        source: String,
    ) -> Result<MemoryEntry, MemoryError> {
        let entry = MemoryEntry {
            id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            content,
            category,
            created_at: chrono::Utc::now().to_rfc3339(),
            source,
            importance: DEFAULT_IMPORTANCE,
            recall_count: 0,
            last_recalled_at: None,
            archived: false,
        };

        // Serialize before acquiring any locks
        let serialized_line = serde_json::to_string(&entry)?;

        // Phase 1: acquire entries lock, push, release
        {
            let mut entries = self.entries.lock().await;
            entries.push(entry.clone());
        }

        // Phase 2: acquire file_path lock, clone path, release (deadlock prevention:
        //          never hold entries lock while acquiring file_path lock)
        let fp = {
            let fp_guard = self.file_path.lock().await;
            fp_guard.clone()
        };

        // Phase 3: offload filesystem I/O to blocking thread pool
        let join_result = tokio::task::spawn_blocking(move || {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&fp)?;
            use std::io::Write;
            writeln!(f, "{}", serialized_line)?;
            Ok::<(), MemoryError>(())
        })
        .await;

        match join_result {
            Ok(Ok(())) => Ok(entry),
            Ok(Err(e)) => {
                eprintln!("ERROR: memory write failed for entry {}: {}", entry.id, e);
                Err(e)
            }
            Err(e) => {
                eprintln!(
                    "ERROR: memory write task panicked for entry {}: {}",
                    entry.id, e
                );
                Err(MemoryError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("spawn_blocking failed: {}", e),
                )))
            }
        }
    }

    /// List all memories.
    pub async fn list(&self) -> Vec<MemoryEntry> {
        self.entries.lock().await.clone()
    }

    /// Delete a memory by ID prefix.
    ///
    /// Returns `Ok(true)` if a memory was deleted, `Ok(false)` if no match found.
    /// Returns `Err(MemoryError)` if the file rewrite fails.
    pub async fn delete(&self, id_prefix: &str) -> Result<bool, MemoryError> {
        let entries_snapshot;
        let mut entries = self.entries.lock().await;
        let before = entries.len();
        entries.retain(|e| !e.id.starts_with(id_prefix));
        if entries.len() < before {
            entries_snapshot = entries.clone();
            drop(entries);
            let fp = self.file_path.lock().await.clone();
            // Offload filesystem I/O — lock already released
            let join_result = tokio::task::spawn_blocking(move || {
                Self::write_all_sync(&fp, &entries_snapshot)
            })
            .await;
            match join_result {
                Ok(Ok(())) => Ok(true),
                Ok(Err(e)) => {
                    eprintln!("ERROR: memory delete write failed: {}", e);
                    Err(e)
                }
                Err(e) => Err(MemoryError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("spawn_blocking failed: {}", e),
                ))),
            }
        } else {
            Ok(false)
        }
    }

    /// Clear all memories.
    ///
    /// Returns the number of cleared memories on success.
    /// Returns `Err(MemoryError)` if the file truncation fails.
    pub async fn clear(&self) -> Result<usize, MemoryError> {
        let count = {
            let mut entries = self.entries.lock().await;
            let count = entries.len();
            entries.clear();
            count
        };
        // Drop entries lock, then do file I/O
        let fp = self.file_path.lock().await.clone();
        let join_result = tokio::task::spawn_blocking(move || {
            std::fs::write(&fp, "")?;
            Ok::<(), MemoryError>(())
        })
        .await;
        match join_result {
            Ok(Ok(())) => Ok(count),
            Ok(Err(e)) => {
                eprintln!("ERROR: memory clear write failed: {}", e);
                Err(e)
            }
            Err(e) => Err(MemoryError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("spawn_blocking failed: {}", e),
            ))),
        }
    }

    /// Build a system prompt fragment from all memories.
    /// Returns None if no memories exist.
    pub async fn build_prompt_fragment(&self) -> Option<String> {
        let entries = self.entries.lock().await;
        if entries.is_empty() {
            return None;
        }

        let mut parts = Vec::new();
        parts.push("# Long-term Memory\n\nThe following are facts, preferences, and decisions remembered from previous conversations. Use them to provide personalized responses.\n".to_string());

        let facts: Vec<&MemoryEntry> = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Fact)).collect();
        let prefs: Vec<&MemoryEntry> = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Preference)).collect();
        let decisions: Vec<&MemoryEntry> = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Decision)).collect();

        if !facts.is_empty() {
            parts.push("## Facts".to_string());
            for e in &facts {
                parts.push(format!("- {}", e.content));
            }
        }
        if !prefs.is_empty() {
            parts.push("\n## Preferences".to_string());
            for e in &prefs {
                parts.push(format!("- {}", e.content));
            }
        }
        if !decisions.is_empty() {
            parts.push("\n## Decisions".to_string());
            for e in &decisions {
                parts.push(format!("- {}", e.content));
            }
        }

        Some(parts.join("\n"))
    }

    /// Archive low-importance memories.
    ///
    /// Uses the decay algorithm to identify memories below the archive threshold.
    /// Moves them to the archive and removes them from active memory.
    ///
    /// # Arguments
    /// * `archive` - The MemoryArchive instance to use
    /// * `config` - Decay configuration parameters
    ///
    /// # Returns
    /// ArchiveResult with IDs of archived memories and cleanup count.
    /// If the file rewrite fails, logs the error but still archives in-memory.
    pub async fn archive_low_importance(
        &self,
        archive: &MemoryArchive,
        config: &DecayConfig,
    ) -> ArchiveResult {
        let mut entries = self.entries.lock().await;

        // Apply decay and find memories to archive
        let to_archive_ids = apply_decay(&mut entries, config);

        if to_archive_ids.is_empty() {
            return ArchiveResult {
                archived_ids: Vec::new(),
                deleted_count: 0,
            };
        }

        // Collect memories to archive
        let to_archive: Vec<MemoryEntry> = entries
            .iter()
            .filter(|e| to_archive_ids.contains(&e.id))
            .cloned()
            .collect();

        // Remove from active memory
        entries.retain(|e| !to_archive_ids.contains(&e.id));

        // Write updated memory file
        let fp = self.file_path.lock().await;
        if let Err(e) = Self::write_all_sync(&fp, &entries) {
            eprintln!("ERROR: memory file rewrite during archive_low_importance failed: {}", e);
        }

        // Add to archive
        let result = archive.archive_memories(to_archive).await;

        eprintln!(
            "Archived {} low-importance memories",
            result.archived_ids.len()
        );

        result
    }

    /// Archive a specific memory by ID.
    ///
    /// Moves the memory to the archive regardless of its importance score.
    /// If the file rewrite fails, logs the error but still returns the archived entry.
    ///
    /// # Arguments
    /// * `id_prefix` - ID prefix of the memory to archive
    /// * `archive` - The MemoryArchive instance to use
    ///
    /// # Returns
    /// The archived memory entry, or None if not found
    pub async fn archive_by_id(
        &self,
        id_prefix: &str,
        archive: &MemoryArchive,
    ) -> Option<MemoryEntry> {
        let mut entries = self.entries.lock().await;

        // Find and remove the memory
        let pos = entries.iter().position(|e| e.id.starts_with(id_prefix))?;
        let memory = entries.remove(pos);

        // Write updated memory file
        let fp = self.file_path.lock().await;
        if let Err(e) = Self::write_all_sync(&fp, &entries) {
            eprintln!("ERROR: memory file rewrite during archive_by_id failed: {}", e);
        }

        // Add to archive
        let archived = archive.archive_memory(memory).await;

        eprintln!("Manually archived memory {}", id_prefix);
        Some(archived)
    }

    /// Restore a memory from the archive.
    ///
    /// Removes the memory from the archive and adds it back to active memory.
    /// Resets the importance to default (0.5) to prevent immediate re-archival.
    /// If the file write fails, logs the error but still returns the restored entry.
    ///
    /// # Arguments
    /// * `id_prefix` - ID prefix of the memory to restore
    /// * `archive` - The MemoryArchive instance to use
    ///
    /// # Returns
    /// The restored memory entry, or None if not found in archive
    pub async fn restore_from_archive(
        &self,
        id_prefix: &str,
        archive: &MemoryArchive,
    ) -> Option<MemoryEntry> {
        // Restore from archive
        let mut memory = archive.restore_memory(id_prefix).await?;

        // Reset importance to prevent immediate re-archival
        memory.importance = DEFAULT_IMPORTANCE;
        memory.archived = false;

        // Add back to active memory
        let mut entries = self.entries.lock().await;
        entries.push(memory.clone());

        // Write to memory file
        match serde_json::to_string(&memory) {
            Ok(line) => {
                use std::io::Write;
                let fp = self.file_path.lock().await;
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&*fp)
                {
                    Ok(mut f) => {
                        if let Err(e) = writeln!(f, "{}", line) {
                            eprintln!("ERROR: memory restore write failed: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "ERROR: failed to open memory file for restore of {}: {}",
                            id_prefix, e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "ERROR: failed to serialize restored memory {}: {}",
                    id_prefix, e
                );
            }
        }

        eprintln!("Restored memory {} from archive", id_prefix);
        Some(memory)
    }

    /// Run periodic memory maintenance.
    ///
    /// This should be called periodically (e.g., daily) to:
    /// 1. Apply decay to all memories
    /// 2. Archive low-importance memories
    /// 3. Clean up archive if needed
    ///
    /// # Arguments
    /// * `archive` - The MemoryArchive instance to use
    /// * `config` - Decay configuration parameters
    ///
    /// # Returns
    /// ArchiveResult with maintenance statistics
    pub async fn run_maintenance(
        &self,
        archive: &MemoryArchive,
        config: &DecayConfig,
    ) -> ArchiveResult {
        eprintln!("Running memory maintenance...");

        // Archive low-importance memories
        let result = self.archive_low_importance(archive, config).await;

        // Run archive cleanup
        let cleanup_count = archive.cleanup().await;

        eprintln!(
            "Maintenance complete: {} archived, {} cleaned up",
            result.archived_ids.len(),
            cleanup_count
        );

        ArchiveResult {
            archived_ids: result.archived_ids,
            deleted_count: result.deleted_count + cleanup_count,
        }
    }

    /// Get memory statistics.
    ///
    /// Returns counts of memories by category and archive status.
    pub async fn stats(&self) -> MemoryStats {
        let entries = self.entries.lock().await;

        let total = entries.len();
        let facts = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Fact)).count();
        let preferences = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Preference)).count();
        let decisions = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Decision)).count();
        let archived = entries.iter().filter(|e| e.archived).count();

        let avg_importance = if total > 0 {
            entries.iter().map(|e| e.importance).sum::<f64>() / total as f64
        } else {
            0.0
        };

        MemoryStats {
            total,
            facts,
            preferences,
            decisions,
            archived,
            avg_importance,
        }
    }
}

/// Memory statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Total number of memories.
    pub total: usize,
    /// Number of fact memories.
    pub facts: usize,
    /// Number of preference memories.
    pub preferences: usize,
    /// Number of decision memories.
    pub decisions: usize,
    /// Number of archived memories.
    pub archived: usize,
    /// Average importance score.
    pub avg_importance: f64,
}

/// Parse a category string into MemoryCategory.
pub fn parse_category(s: &str) -> MemoryCategory {
    match s.to_lowercase().as_str() {
        "preference" | "pref" => MemoryCategory::Preference,
        "decision" | "dec" => MemoryCategory::Decision,
        _ => MemoryCategory::Fact,
    }
}
