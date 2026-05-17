use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

// ── Data Structures ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionIndex {
    pub id: String,
    pub cwd: String,
    pub model: String,
    pub started_at: String,
    pub ended_at: String,
    pub turn_count: i32,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub session_id: String,
    pub snippet: String,
    pub rank: f64,
    pub timestamp: String,
    pub cwd: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageRecord {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

// ── Database Path Helper ─────────────────────────────────────────────────────

fn db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(home).join(".baoclaw");
    std::fs::create_dir_all(&dir).ok();
    dir.join("cross_session.db")
}

/// Strip HTML tags from a string (simple version: remove `<...>`).
fn strip_html(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

// ── CrossSessionDb ───────────────────────────────────────────────────────────

pub struct CrossSessionDb {
    conn: Mutex<Connection>,
}

impl CrossSessionDb {
    /// Open (or create) the database and ensure all tables exist.
    pub fn new() -> Result<Self, String> {
        let path = db_path();
        let conn = Connection::open(&path)
            .map_err(|e| format!("Failed to open cross_session.db at {:?}: {}", path, e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("PRAGMA failed: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                 id          TEXT PRIMARY KEY,
                 cwd         TEXT    NOT NULL DEFAULT '',
                 model       TEXT    NOT NULL DEFAULT '',
                 started_at  TEXT    NOT NULL DEFAULT '',
                 ended_at    TEXT    NOT NULL DEFAULT '',
                 turn_count  INTEGER NOT NULL DEFAULT 0,
                 cost_usd    REAL    NOT NULL DEFAULT 0.0
             );

             CREATE TABLE IF NOT EXISTS messages (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id  TEXT    NOT NULL REFERENCES sessions(id),
                 role        TEXT    NOT NULL,
                 content     TEXT    NOT NULL DEFAULT '',
                 timestamp   TEXT    NOT NULL DEFAULT ''
             );

             CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
                 USING fts5(content, content=messages, content_rowid=id);",
        )
        .map_err(|e| format!("Create tables failed: {}", e))?;

        // Keep the FTS index in sync via triggers (if they do not exist yet).
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                 INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
             END;

             CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                 INSERT INTO messages_fts(messages_fts, rowid, content)
                     VALUES('delete', old.id, old.content);
             END;

             CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
                 INSERT INTO messages_fts(messages_fts, rowid, content)
                     VALUES('delete', old.id, old.content);
                 INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
             END;",
        )
        .map_err(|e| format!("Create triggers failed: {}", e))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Index a full session: insert the session row then each message (into FTS via trigger).
    pub fn index_session(&self, summary: SessionIndex) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT OR REPLACE INTO sessions (id, cwd, model, started_at, ended_at, turn_count, cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                summary.id,
                summary.cwd,
                summary.model,
                summary.started_at,
                summary.ended_at,
                summary.turn_count,
                summary.cost_usd,
            ],
        )
        .map_err(|e| format!("Insert session failed: {}", e))?;

        Ok(())
    }

    /// Insert a single message for a session (FTS trigger fires automatically).
    pub fn index_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        timestamp: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let cleaned = strip_html(content);

        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_id, role, cleaned, timestamp],
        )
        .map_err(|e| format!("Insert message failed: {}", e))?;

        Ok(())
    }

    /// Full-text search using FTS5 with bm25 ranking.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        let sql = format!(
            "SELECT m.session_id,
                    snippet(messages_fts, '[', ']', '...', 1, 32) AS snippet,
                    bm25(messages_fts) AS rank,
                    m.timestamp,
                    s.cwd
             FROM messages_fts
             JOIN messages m ON m.id = messages_fts.rowid
             JOIN sessions  s ON s.id = m.session_id
             WHERE messages_fts MATCH ?1
             ORDER BY rank
             LIMIT {}",
            limit
        );

        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let rows = stmt.query_map(params![query], |row| {
            Ok(SearchResult {
                session_id: row.get(0)?,
                snippet: row.get(1)?,
                rank: row.get(2)?,
                timestamp: row.get(3)?,
                cwd: row.get(4)?,
            })
        });

        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// Return the most recent N sessions ordered by started_at descending.
    pub fn get_recent_sessions(&self, limit: usize) -> Vec<SessionIndex> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        let mut stmt = match conn.prepare(
            "SELECT id, cwd, model, started_at, ended_at, turn_count, cost_usd
             FROM sessions
             ORDER BY started_at DESC
             LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let rows = stmt.query_map(params![limit], |row| {
            Ok(SessionIndex {
                id: row.get(0)?,
                cwd: row.get(1)?,
                model: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                turn_count: row.get(5)?,
                cost_usd: row.get(6)?,
            })
        });

        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// FTS5 search that also returns one message before and after each hit for context.
    pub fn search_with_context(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        // First, get the base search results.
        let base = self.search(query, limit);
        if base.is_empty() {
            return vec![];
        }

        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return base,
        };

        let mut enriched = Vec::with_capacity(base.len());
        for sr in &base {
            // Find the rowid of the matched message.
            let msg_rowid: Option<i64> = conn
                .query_row(
                    "SELECT id FROM messages
                     WHERE session_id = ?1 AND timestamp = ?2
                     ORDER BY id DESC LIMIT 1",
                    params![sr.session_id, sr.timestamp],
                    |row| row.get(0),
                )
                .ok();

            let mut context_parts: Vec<String> = Vec::new();

            if let Some(rid) = msg_rowid {
                // Previous message
                if let Ok(prev) = conn.query_row(
                    "SELECT role, content FROM messages WHERE id = ?1",
                    params![rid - 1],
                    |row| {
                        let role: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        Ok(format!("[{}]: {}", role, content))
                    },
                ) {
                    context_parts.push(prev);
                }

                context_parts.push(format!(">>> {}", sr.snippet));

                // Next message
                if let Ok(next) = conn.query_row(
                    "SELECT role, content FROM messages WHERE id = ?1",
                    params![rid + 1],
                    |row| {
                        let role: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        Ok(format!("[{}]: {}", role, content))
                    },
                ) {
                    context_parts.push(next);
                }
            } else {
                context_parts.push(sr.snippet.clone());
            }

            enriched.push(SearchResult {
                session_id: sr.session_id.clone(),
                snippet: context_parts.join("\n---\n"),
                rank: sr.rank,
                timestamp: sr.timestamp.clone(),
                cwd: sr.cwd.clone(),
            });
        }

        enriched
    }

    /// Return every message belonging to a given session.
    pub fn get_session_messages(&self, session_id: &str) -> Vec<MessageRecord> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        let mut stmt = match conn.prepare(
            "SELECT id, session_id, role, content, timestamp
             FROM messages
             WHERE session_id = ?1
             ORDER BY id ASC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let rows = stmt.query_map(params![session_id], |row| {
            Ok(MessageRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                timestamp: row.get(4)?,
            })
        });

        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }
}
