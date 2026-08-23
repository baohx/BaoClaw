//! Session persistence to disk (JSON files) for crash recovery.
//!
//! Each session's conversation state is persisted to `~/.baoclaw/sessions/<id>.json`
//! using atomic write (tmp + rename). A registry index file tracks all sessions.
//!
//! ## Storage Layout
//! ```text
//! ~/.baoclaw/sessions/
//! ├── registry.json          # session index (id/cwd/created_at/last_active)
//! ├── <session-id-1>.json    # single session full state (messages + metadata)
//! ├── <session-id-2>.json
//! └── archive/               # archived sessions (>7 days inactive)
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::message::Message;

/// Maximum age (in days) before a session is archived.
const DEFAULT_ARCHIVE_AGE_DAYS: u64 = 7;

/// One entry in the registry index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub session_id: String,
    pub cwd: String,
    pub created_at: String,
    pub last_active: String,
}

/// The full registry index (serialized as registry.json).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionRegistryIndex {
    pub sessions: Vec<RegistryEntry>,
}

/// The persisted state of a single session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedSession {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub created_at: String,
    pub last_active: String,
    pub messages: Vec<Message>,
    /// Session memory summary text (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_summary: Option<String>,
}

/// Compute the default sessions directory: `~/.baoclaw/sessions/`.
pub fn default_sessions_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".baoclaw").join("sessions")
}

/// Ensure a directory exists, creating it (and parents) if needed.
fn ensure_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

/// Atomically write content to a file.
///
/// Writes to `<path>.tmp` first, then renames to `<path>`.
/// This prevents corruption if the process is killed mid-write.
/// The rename is atomic on the same filesystem (POSIX guarantee).
pub fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Path for a session's JSON state file.
fn session_file_path(sessions_dir: &Path, session_id: &str) -> PathBuf {
    sessions_dir.join(format!("{}.json", session_id))
}

/// Path for the registry index file.
fn registry_file_path(sessions_dir: &Path) -> PathBuf {
    sessions_dir.join("registry.json")
}

/// Path for the archive subdirectory.
fn archive_dir(sessions_dir: &Path) -> PathBuf {
    sessions_dir.join("archive")
}

// ── Registry Index Operations ──

/// Load the registry index from disk. Returns empty if missing.
pub fn load_registry(sessions_dir: &Path) -> SessionRegistryIndex {
    let path = registry_file_path(sessions_dir);
    match fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or_else(|e| {
                eprintln!("[session-persist] WARNING: registry.json corrupted: {}. Starting fresh.", e);
                SessionRegistryIndex::default()
            })
        }
        Err(_) => SessionRegistryIndex::default(),
    }
}

/// Save the registry index to disk (atomic write).
pub fn save_registry(sessions_dir: &Path, index: &SessionRegistryIndex) -> io::Result<()> {
    let path = registry_file_path(sessions_dir);
    let json = serde_json::to_string_pretty(index)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write(&path, &json)
}

/// Update or insert a registry entry, then persist the index.
pub fn upsert_registry_entry(
    sessions_dir: &Path,
    session_id: &str,
    cwd: &str,
    created_at: &str,
    last_active: &str,
) -> io::Result<()> {
    ensure_dir(sessions_dir)?;
    let mut index = load_registry(sessions_dir);
    let now = last_active.to_string();

    if let Some(entry) = index.sessions.iter_mut().find(|e| e.session_id == session_id) {
        entry.cwd = cwd.to_string();
        entry.last_active = now;
    } else {
        index.sessions.push(RegistryEntry {
            session_id: session_id.to_string(),
            cwd: cwd.to_string(),
            created_at: created_at.to_string(),
            last_active: now,
        });
    }

    save_registry(sessions_dir, &index)
}

// ── Session State Persistence ──

/// Persist a session's full state to disk (atomic write).
///
/// Serializes messages + metadata to `<session_id>.json`.
/// Also updates the registry index's `last_active`.
pub fn persist_session_state(sessions_dir: &Path, state: &PersistedSession) -> io::Result<()> {
    ensure_dir(sessions_dir)?;

    // 1. Write the session state file
    let path = session_file_path(sessions_dir, &state.session_id);
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write(&path, &json)?;

    // 2. Update registry index
    upsert_registry_entry(
        sessions_dir,
        &state.session_id,
        &state.cwd,
        &state.created_at,
        &state.last_active,
    )?;

    Ok(())
}

/// Load a session's persisted state from disk.
///
/// Returns `None` if the file doesn't exist.
/// Logs a warning and returns `None` if the file is corrupted.
pub fn load_session_state(sessions_dir: &Path, session_id: &str) -> Option<PersistedSession> {
    let path = session_file_path(sessions_dir, session_id);
    match fs::read_to_string(&path) {
        Ok(content) => {
            match serde_json::from_str::<PersistedSession>(&content) {
                Ok(state) => Some(state),
                Err(e) => {
                    eprintln!(
                        "[session-persist] WARNING: session {} state corrupted: {}. Skipping.",
                        session_id, e
                    );
                    None
                }
            }
        }
        Err(_) => None,
    }
}

// ── Archive Stale Sessions ──

/// Move sessions inactive for more than `max_age_days` to the archive/ subdirectory.
///
/// This only operates on files on disk — in-memory sessions are not affected
/// (the caller should handle evicting stale sessions from the registry).
///
/// Returns the list of archived session IDs.
pub fn archive_stale_sessions(sessions_dir: &Path, max_age_days: u64) -> io::Result<Vec<String>> {
    let index = load_registry(sessions_dir);
    let now = Utc::now();
    let archive = archive_dir(sessions_dir);
    let mut archived = Vec::new();

    for entry in &index.sessions {
        // Parse last_active timestamp
        let last_active = entry.last_active
            .parse::<DateTime<Utc>>()
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&entry.last_active, "%Y-%m-%dT%H:%M:%S%.fZ").map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc)))
            .ok();

        let last_active = match last_active {
            Some(dt) => dt,
            None => continue, // Skip unparseable timestamps
        };

        let age = now.signed_duration_since(last_active);
        if age.num_days() > max_age_days as i64 {
            // Move the session file to archive/
            let src = session_file_path(sessions_dir, &entry.session_id);
            if src.exists() {
                ensure_dir(&archive)?;
                let dst = archive.join(format!("{}.json", entry.session_id));
                if let Err(e) = fs::rename(&src, &dst) {
                    eprintln!(
                        "[session-persist] WARNING: failed to archive {}: {}",
                        entry.session_id, e
                    );
                    continue;
                }
                archived.push(entry.session_id.clone());
            }
        }
    }

    // Remove archived sessions from the registry index
    if !archived.is_empty() {
        let mut new_index = index;
        new_index.sessions.retain(|e| !archived.contains(&e.session_id));
        save_registry(sessions_dir, &new_index)?;
    }

    Ok(archived)
}

/// Convenience: archive sessions with the default 7-day threshold.
pub fn archive_stale_default(sessions_dir: &Path) -> io::Result<Vec<String>> {
    archive_stale_sessions(sessions_dir, DEFAULT_ARCHIVE_AGE_DAYS)
}

// ── Remove a session from disk ──

/// Delete a session's state file and remove it from the registry index.
pub fn delete_session(sessions_dir: &Path, session_id: &str) -> io::Result<()> {
    let path = session_file_path(sessions_dir, session_id);
    let _ = fs::remove_file(&path); // ignore error if not exists

    let mut index = load_registry(sessions_dir);
    index.sessions.retain(|e| e.session_id != session_id);
    save_registry(sessions_dir, &index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_dir() -> TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    #[test]
    fn test_atomic_write_basic() {
        let dir = make_test_dir();
        let path = dir.path().join("test.json");
        atomic_write(&path, r#"{"hello": "world"}"#).expect("write failed");
        let content = fs::read_to_string(&path).expect("read failed");
        assert!(content.contains("hello"));
    }

    #[test]
    fn test_atomic_write_no_tmp_left() {
        let dir = make_test_dir();
        let path = dir.path().join("test.json");
        atomic_write(&path, r#"{"v": 1}"#).expect("write failed");
        let tmp = dir.path().join("test.json.tmp");
        assert!(!tmp.exists(), "temp file should be gone after rename");
    }

    #[test]
    fn test_persist_and_load_session() {
        let dir = make_test_dir();
        let sessions_dir = dir.path().join("sessions");

        let state = PersistedSession {
            session_id: "test-123".to_string(),
            cwd: "/tmp/project".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            last_active: "2025-01-02T00:00:00Z".to_string(),
            messages: Vec::new(),
            memory_summary: Some("Test summary".to_string()),
        };

        persist_session_state(&sessions_dir, &state).expect("persist failed");

        // Verify file exists
        let file = sessions_dir.join("test-123.json");
        assert!(file.exists());

        // Verify registry exists
        let reg = sessions_dir.join("registry.json");
        assert!(reg.exists());

        // Load back
        let loaded = load_session_state(&sessions_dir, "test-123").expect("load failed");
        assert_eq!(loaded.session_id, "test-123");
        assert_eq!(loaded.model, "claude-sonnet-4-20250514");
        assert_eq!(loaded.memory_summary.as_deref(), Some("Test summary"));
    }

    #[test]
    fn test_load_missing_session() {
        let dir = make_test_dir();
        let sessions_dir = dir.path().join("sessions");
        ensure_dir(&sessions_dir).unwrap();
        let loaded = load_session_state(&sessions_dir, "nonexistent");
        assert!(loaded.is_none());
    }

    #[test]
    fn test_registry_upsert_and_update() {
        let dir = make_test_dir();
        let sessions_dir = dir.path().join("sessions");

        // First insert
        upsert_registry_entry(&sessions_dir, "s1", "/a", "2025-01-01T00:00:00Z", "2025-01-01T00:00:00Z").unwrap();
        let idx = load_registry(&sessions_dir);
        assert_eq!(idx.sessions.len(), 1);

        // Second insert
        upsert_registry_entry(&sessions_dir, "s2", "/b", "2025-01-02T00:00:00Z", "2025-01-02T00:00:00Z").unwrap();
        let idx = load_registry(&sessions_dir);
        assert_eq!(idx.sessions.len(), 2);

        // Update existing (should not add new)
        upsert_registry_entry(&sessions_dir, "s1", "/a", "2025-01-01T00:00:00Z", "2025-01-03T00:00:00Z").unwrap();
        let idx = load_registry(&sessions_dir);
        assert_eq!(idx.sessions.len(), 2);
        let s1 = idx.sessions.iter().find(|e| e.session_id == "s1").unwrap();
        assert_eq!(s1.last_active, "2025-01-03T00:00:00Z");
    }

    #[test]
    fn test_delete_session() {
        let dir = make_test_dir();
        let sessions_dir = dir.path().join("sessions");

        let state = PersistedSession {
            session_id: "del-1".to_string(),
            cwd: "/tmp".to_string(),
            model: "m".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            last_active: "2025-01-01T00:00:00Z".to_string(),
            messages: Vec::new(),
            memory_summary: None,
        };
        persist_session_state(&sessions_dir, &state).unwrap();

        // Verify exists
        assert!(load_session_state(&sessions_dir, "del-1").is_some());

        // Delete
        delete_session(&sessions_dir, "del-1").unwrap();
        assert!(load_session_state(&sessions_dir, "del-1").is_none());

        // Registry should be empty
        let idx = load_registry(&sessions_dir);
        assert!(idx.sessions.is_empty());
    }

    #[test]
    fn test_archive_stale() {
        let dir = make_test_dir();
        let sessions_dir = dir.path().join("sessions");

        // Create an old session (30 days ago)
        let old_state = PersistedSession {
            session_id: "old-1".to_string(),
            cwd: "/tmp".to_string(),
            model: "m".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            last_active: "2025-01-01T00:00:00Z".to_string(),
            messages: Vec::new(),
            memory_summary: None,
        };
        persist_session_state(&sessions_dir, &old_state).unwrap();

        // Create a recent session (now)
        let now_iso = Utc::now().to_rfc3339();
        let recent_state = PersistedSession {
            session_id: "recent-1".to_string(),
            cwd: "/tmp".to_string(),
            model: "m".to_string(),
            created_at: now_iso.clone(),
            last_active: now_iso,
            messages: Vec::new(),
            memory_summary: None,
        };
        persist_session_state(&sessions_dir, &recent_state).unwrap();

        // Archive with 7-day threshold
        let archived = archive_stale_sessions(&sessions_dir, 7).unwrap();
        assert_eq!(archived, vec!["old-1".to_string()]);

        // old-1 should be moved to archive/
        let archive_path = sessions_dir.join("archive").join("old-1.json");
        assert!(archive_path.exists());

        // old-1 should no longer be in main dir
        assert!(load_session_state(&sessions_dir, "old-1").is_none());

        // recent-1 should still be in main dir
        assert!(load_session_state(&sessions_dir, "recent-1").is_some());

        // Registry should only have recent-1
        let idx = load_registry(&sessions_dir);
        assert_eq!(idx.sessions.len(), 1);
        assert_eq!(idx.sessions[0].session_id, "recent-1");
    }

    #[test]
    fn test_overwrite_persist() {
        let dir = make_test_dir();
        let sessions_dir = dir.path().join("sessions");

        // Write v1
        let mut state = PersistedSession {
            session_id: "s".to_string(),
            cwd: "/tmp".to_string(),
            model: "m1".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            last_active: "2025-01-01T00:00:00Z".to_string(),
            messages: Vec::new(),
            memory_summary: None,
        };
        persist_session_state(&sessions_dir, &state).unwrap();

        // Write v2 (overwrite)
        state.model = "m2".to_string();
        state.last_active = "2025-01-02T00:00:00Z".to_string();
        persist_session_state(&sessions_dir, &state).unwrap();

        // Should have only 1 file, 1 registry entry
        let loaded = load_session_state(&sessions_dir, "s").unwrap();
        assert_eq!(loaded.model, "m2");

        let idx = load_registry(&sessions_dir);
        assert_eq!(idx.sessions.len(), 1);
    }
}
