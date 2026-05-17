//! Session-level rolling summary ("meeting notes").
//!
//! Maintained across the lifetime of a session and persisted to
//! `~/.baoclaw/sessions/{session_id}.memory.md`.
//!
//! Inspired by Claude Code's session memory mechanism:
//! - Updated every N turns via background API call during the session
//! - Loaded instantly at startup (no on-demand summarization)
//! - Used by session_memory_compact for zero-cost compaction

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// Minimum turns before first summary generation.
const FIRST_UPDATE_THRESHOLD: usize = 6;

/// Number of new messages between summary updates.
const UPDATE_INTERVAL: usize = 10;

/// Maximum summary length (chars).  Summaries exceeding this are truncated.
const MAX_SUMMARY_CHARS: usize = 8000;

/// Session-level rolling summary persisted to disk.
///
/// All mutable state is behind `Mutex` so that `&SessionMemory` is enough
/// for both reads and writes — safe to share behind `Arc<SessionMemory>`.
pub struct SessionMemory {
    file_path: PathBuf,
    content: Mutex<String>,
    last_update_count: Mutex<usize>,
}

impl SessionMemory {
    /// Compute the file path for a given session ID.
    pub fn path_for(session_id: &str) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join(".baoclaw")
            .join("sessions")
            .join(format!("{}.memory.md", session_id))
    }

    /// Load an existing session memory file. Returns empty string if missing.
    pub fn load(session_id: &str) -> Self {
        let file_path = Self::path_for(session_id);
        let _ = fs::create_dir_all(file_path.parent().unwrap_or(&file_path));
        let content = fs::read_to_string(&file_path).unwrap_or_default();

        Self {
            file_path,
            content: Mutex::new(content),
            last_update_count: Mutex::new(0),
        }
    }

    /// Return the current summary text.
    pub fn get(&self) -> String {
        self.content.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Whether a non-trivial summary is available.
    pub fn is_available(&self) -> bool {
        let guard = self.content.lock().unwrap_or_else(|e| e.into_inner());
        let trimmed = guard.trim();
        !trimmed.is_empty() && trimmed.len() > 20
    }

    /// Whether enough new messages have arrived to warrant an update.
    pub fn should_update(&self, message_count: usize) -> bool {
        let guard = self.content.lock().unwrap_or_else(|e| e.into_inner());
        let current = guard.trim();
        if current.is_empty() || current.len() <= 20 {
            // No real summary yet — update after first few messages.
            drop(guard);
            message_count >= FIRST_UPDATE_THRESHOLD
        } else {
            let last = *self.last_update_count.lock().unwrap_or_else(|e| e.into_inner());
            drop(guard);
            message_count >= last + UPDATE_INTERVAL
        }
    }

    /// Write a new summary to memory (and persist to disk).
    pub fn update(&self, summary: String) {
        let truncated = if summary.len() > MAX_SUMMARY_CHARS {
            format!("{}...\n\n[Summary truncated at {} chars]",
                &summary.chars().take(MAX_SUMMARY_CHARS).collect::<String>(),
                summary.len())
        } else {
            summary
        };
        let mut guard = self.content.lock().unwrap_or_else(|e| e.into_inner());
        *guard = truncated;
        let _ = fs::write(&self.file_path, guard.as_bytes());
    }

    /// Record the current message count so `should_update` can track deltas.
    pub fn set_message_count(&self, count: usize) {
        let mut guard = self.last_update_count.lock().unwrap_or_else(|e| e.into_inner());
        *guard = count;
    }

    /// Clear the session memory.
    pub fn clear(&self) {
        let mut guard = self.content.lock().unwrap_or_else(|e| e.into_inner());
        guard.clear();
        let _ = fs::write(&self.file_path, "");
    }

    /// Path to the backing file (for diagnostics).
    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_update_empty() {
        let sm = SessionMemory::load("__test_unit__");
        assert!(!sm.should_update(5));
        assert!(sm.should_update(6));
        sm.clear();
    }

    #[test]
    fn test_update_and_get() {
        let sm = SessionMemory::load("__test_unit_2__");
        let summary = "# Session Memory\n- Did stuff".to_string();
        sm.update(summary.clone());
        assert_eq!(sm.get(), summary);
        sm.clear();
        assert!(sm.get().is_empty());
    }

    #[test]
    fn test_is_available() {
        let sm = SessionMemory::load("__test_unit_3__");
        sm.update("# Memory\nThis is a real summary with enough content.".to_string());
        assert!(sm.is_available());
        sm.update("".to_string());
        assert!(!sm.is_available());
        sm.clear();
    }

    #[test]
    fn test_set_message_count() {
        let sm = SessionMemory::load("__test_unit_4__");
        sm.update("# Memory\nThis is a real summary with enough content to pass the threshold.".to_string());
        sm.set_message_count(10);
        assert!(!sm.should_update(19));
        assert!(sm.should_update(20));
        sm.clear();
    }

    #[test]
    fn test_truncation() {
        let sm = SessionMemory::load("__test_unit_5__");
        let long = "X".repeat(10_000);
        sm.update(long.clone());
        assert!(sm.get().len() < long.len());
        assert!(sm.get().contains("[Summary truncated"));
        sm.clear();
    }
}
