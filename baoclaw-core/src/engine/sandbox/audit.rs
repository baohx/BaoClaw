//! Sandbox audit logging.
//!
//! Records all sandbox decisions, file changes, and security events
//! to a SQLite database at `~/.baoclaw/sandbox_audit.db`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Types of auditable sandbox events.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum AuditEvent {
    /// A sandbox profile was selected for execution.
    ProfileSelected {
        tool: String,
        profile: String,
        reason: String,
    },
    /// A permission escalation was requested.
    EscalationRequested {
        tool: String,
        current_profile: String,
        requested_action: String,
        target: String,
    },
    /// A permission escalation was granted (temporary or permanent).
    EscalationGranted {
        tool: String,
        from_profile: String,
        to_profile: String,
        granted_by: String,
        duration: String, // "temporary" or "permanent"
    },
    /// An execution was blocked by the sandbox.
    ExecutionBlocked {
        tool: String,
        profile: String,
        reason: String,
    },
    /// A file was written inside the sandbox.
    FileWritten {
        path: String,
        tool: String,
        profile: String,
    },
    /// A tool was executed under a specific profile.
    ToolExecuted {
        tool: String,
        profile: String,
        exit_code: i32,
        duration_ms: u64,
    },
}

/// Audit log backed by SQLite.
pub struct AuditLog {
    db: Mutex<rusqlite::Connection>,
}

impl AuditLog {
    /// Open or create the audit database at the default path.
    pub fn open_default() -> Result<Self, String> {
        Self::open(&audit_db_path())
    }

    /// Open or create the audit database at the given path.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create audit dir: {}", e))?;
        }

        let conn = rusqlite::Connection::open(path)
            .map_err(|e| format!("Failed to open audit DB: {}", e))?;

        // Enable WAL for concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| format!("Failed to set WAL: {}", e))?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                tool TEXT NOT NULL,
                profile TEXT NOT NULL DEFAULT '',
                details TEXT NOT NULL DEFAULT '{}',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events(timestamp);
            CREATE INDEX IF NOT EXISTS idx_audit_type ON audit_events(event_type);
            CREATE INDEX IF NOT EXISTS idx_audit_tool ON audit_events(tool);"
        ).map_err(|e| format!("Failed to create audit tables: {}", e))?;

        Ok(Self { db: Mutex::new(conn) })
    }

    /// Record an audit event.
    pub fn record(&self, event: &AuditEvent) -> Result<(), String> {
        let conn = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let (event_type, tool, profile) = match event {
            AuditEvent::ProfileSelected { tool, profile, .. } => ("profile_selected", tool.as_str(), profile.as_str()),
            AuditEvent::EscalationRequested { tool, current_profile, .. } => ("escalation_requested", tool.as_str(), current_profile.as_str()),
            AuditEvent::EscalationGranted { tool, to_profile, .. } => ("escalation_granted", tool.as_str(), to_profile.as_str()),
            AuditEvent::ExecutionBlocked { tool, profile, .. } => ("execution_blocked", tool.as_str(), profile.as_str()),
            AuditEvent::FileWritten { tool, profile, .. } => ("file_written", tool.as_str(), profile.as_str()),
            AuditEvent::ToolExecuted { tool, profile, .. } => ("tool_executed", tool.as_str(), profile.as_str()),
        };

        let details = serde_json::to_string(event)
            .map_err(|e| format!("Serialization error: {}", e))?;

        conn.execute(
            "INSERT INTO audit_events (timestamp, event_type, tool, profile, details) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![timestamp, event_type, tool, profile, details],
        ).map_err(|e| format!("Insert error: {}", e))?;

        Ok(())
    }

    /// Query recent audit events.
    pub fn query_recent(&self, limit: u32) -> Result<Vec<AuditEventRecord>, String> {
        let conn = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, timestamp, event_type, tool, profile, details FROM audit_events ORDER BY id DESC LIMIT ?1"
        ).map_err(|e| format!("Prepare error: {}", e))?;

        let records = stmt.query_map([limit], |row| {
            Ok(AuditEventRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                event_type: row.get(2)?,
                tool: row.get(3)?,
                profile: row.get(4)?,
                details: row.get(5)?,
            })
        }).map_err(|e| format!("Query error: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(records)
    }

    /// Query audit events by type.
    pub fn query_by_type(&self, event_type: &str, limit: u32) -> Result<Vec<AuditEventRecord>, String> {
        let conn = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, timestamp, event_type, tool, profile, details FROM audit_events WHERE event_type = ?1 ORDER BY id DESC LIMIT ?2"
        ).map_err(|e| format!("Prepare error: {}", e))?;

        let records = stmt.query_map(rusqlite::params![event_type, limit], |row| {
            Ok(AuditEventRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                event_type: row.get(2)?,
                tool: row.get(3)?,
                profile: row.get(4)?,
                details: row.get(5)?,
            })
        }).map_err(|e| format!("Query error: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(records)
    }

    /// Query audit events for a specific tool.
    pub fn query_by_tool(&self, tool: &str, limit: u32) -> Result<Vec<AuditEventRecord>, String> {
        let conn = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, timestamp, event_type, tool, profile, details FROM audit_events WHERE tool = ?1 ORDER BY id DESC LIMIT ?2"
        ).map_err(|e| format!("Prepare error: {}", e))?;

        let records = stmt.query_map(rusqlite::params![tool, limit], |row| {
            Ok(AuditEventRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                event_type: row.get(2)?,
                tool: row.get(3)?,
                profile: row.get(4)?,
                details: row.get(5)?,
            })
        }).map_err(|e| format!("Query error: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(records)
    }

    /// Count events by type.
    pub fn count_by_type(&self) -> Result<Vec<(String, u32)>, String> {
        let conn = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT event_type, COUNT(*) FROM audit_events GROUP BY event_type"
        ).map_err(|e| format!("Prepare error: {}", e))?;

        let counts = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        }).map_err(|e| format!("Query error: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(counts)
    }

    /// Purge events older than the given number of days.
    pub fn purge_older_than_days(&self, days: u32) -> Result<u32, String> {
        let conn = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;

        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub((days as u64) * 86400);

        let deleted = conn.execute(
            "DELETE FROM audit_events WHERE timestamp < ?1",
            rusqlite::params![cutoff],
        ).map_err(|e| format!("Delete error: {}", e))?;

        Ok(deleted as u32)
    }
}

/// A record from the audit log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEventRecord {
    pub id: i64,
    pub timestamp: i64,
    pub event_type: String,
    pub tool: String,
    pub profile: String,
    pub details: String,
}

/// Returns the audit database path: ~/.baoclaw/sandbox_audit.db
pub fn audit_db_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".baoclaw").join("sandbox_audit.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn audit_in(dir: &std::path::Path) -> PathBuf {
        dir.join("test_audit.db")
    }

    #[test]
    fn test_record_and_query() {
        let dir = TempDir::new().unwrap();
        let path = audit_in(dir.path());
        let log = AuditLog::open(&path).unwrap();

        let event = AuditEvent::ProfileSelected {
            tool: "FileRead".into(),
            profile: "read_only".into(),
            reason: "auto-detected".into(),
        };
        log.record(&event).unwrap();

        let records = log.query_recent(10).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].event_type, "profile_selected");
        assert_eq!(records[0].tool, "FileRead");
        assert_eq!(records[0].profile, "read_only");
    }

    #[test]
    fn test_multiple_event_types() {
        let dir = TempDir::new().unwrap();
        let path = audit_in(dir.path());
        let log = AuditLog::open(&path).unwrap();

        log.record(&AuditEvent::ProfileSelected {
            tool: "Bash".into(),
            profile: "web_dev".into(),
            reason: "npm command".into(),
        }).unwrap();

        log.record(&AuditEvent::ExecutionBlocked {
            tool: "Bash".into(),
            profile: "read_only".into(),
            reason: "network not allowed".into(),
        }).unwrap();

        log.record(&AuditEvent::ToolExecuted {
            tool: "FileRead".into(),
            profile: "read_only".into(),
            exit_code: 0,
            duration_ms: 150,
        }).unwrap();

        let counts = log.count_by_type().unwrap();
        assert!(counts.iter().any(|(t, c)| t == "profile_selected" && *c == 1));
        assert!(counts.iter().any(|(t, c)| t == "execution_blocked" && *c == 1));
        assert!(counts.iter().any(|(t, c)| t == "tool_executed" && *c == 1));
    }

    #[test]
    fn test_query_by_type() {
        let dir = TempDir::new().unwrap();
        let path = audit_in(dir.path());
        let log = AuditLog::open(&path).unwrap();

        log.record(&AuditEvent::ExecutionBlocked {
            tool: "Bash".into(),
            profile: "read_only".into(),
            reason: "blocked".into(),
        }).unwrap();

        log.record(&AuditEvent::ExecutionBlocked {
            tool: "FileWrite".into(),
            profile: "read_only".into(),
            reason: "blocked2".into(),
        }).unwrap();

        let blocked = log.query_by_type("execution_blocked", 10).unwrap();
        assert_eq!(blocked.len(), 2);
    }

    #[test]
    fn test_query_by_tool() {
        let dir = TempDir::new().unwrap();
        let path = audit_in(dir.path());
        let log = AuditLog::open(&path).unwrap();

        log.record(&AuditEvent::ProfileSelected {
            tool: "Bash".into(),
            profile: "web_dev".into(),
            reason: "test".into(),
        }).unwrap();

        log.record(&AuditEvent::ProfileSelected {
            tool: "FileRead".into(),
            profile: "read_only".into(),
            reason: "test".into(),
        }).unwrap();

        let bash_events = log.query_by_tool("Bash", 10).unwrap();
        assert_eq!(bash_events.len(), 1);
        assert_eq!(bash_events[0].tool, "Bash");
    }

    #[test]
    fn test_purge() {
        let dir = TempDir::new().unwrap();
        let path = audit_in(dir.path());
        let log = AuditLog::open(&path).unwrap();

        // Record an event with timestamp 0 (should be purged if days > 0)
        let conn = log.db.lock().unwrap();
        conn.execute(
            "INSERT INTO audit_events (timestamp, event_type, tool, profile, details) VALUES (0, 'test', 'test', '', '{}')",
            [],
        ).unwrap();
        drop(conn);

        let deleted = log.purge_older_than_days(1).unwrap();
        assert!(deleted >= 1);
    }
}
