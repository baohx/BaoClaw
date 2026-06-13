//! Telemetry data collector backed by SQLite.
//!
//! The `TelemetryCollector` records per-turn and per-session metrics
//! to `~/.baoclaw/telemetry.db`. It provides aggregated queries for
//! usage stats, tool usage, daily stats, and session history.

use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;

use super::types::{
    DailyStats, SessionSnapshot, ToolUsageStat, UsageStats,
};

/// Default path for the telemetry database.
fn default_db_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".baoclaw").join("telemetry.db")
}

/// Collects and queries telemetry data from a local SQLite database.
pub struct TelemetryCollector {
    conn: Mutex<Connection>,
}

impl TelemetryCollector {
    /// Create a new collector using the default database path.
    /// Initializes the schema if needed.
    pub fn new() -> Result<Self, String> {
        Self::with_path(&default_db_path())
    }

    /// Create a new collector with a custom database path.
    /// Initializes the schema if needed.
    pub fn with_path(path: &PathBuf) -> Result<Self, String> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create telemetry directory: {}", e))?;
        }

        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open telemetry database: {}", e))?;

        let collector = Self {
            conn: Mutex::new(conn),
        };
        collector.init_schema()?;
        Ok(collector)
    }

    /// Initialize the database schema (idempotent).
    fn init_schema(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS turns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                tokens_in INTEGER NOT NULL DEFAULT 0,
                tokens_out INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL NOT NULL DEFAULT 0.0,
                response_time_ms INTEGER NOT NULL DEFAULT 0,
                tools_used TEXT NOT NULL DEFAULT '[]'
            );

            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                start_time INTEGER NOT NULL,
                end_time INTEGER,
                turns INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                total_cost REAL NOT NULL DEFAULT 0.0,
                tools_used INTEGER NOT NULL DEFAULT 0,
                files_changed INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_turns_session
                ON turns(session_id);
            CREATE INDEX IF NOT EXISTS idx_turns_timestamp
                ON turns(timestamp);
            CREATE INDEX IF NOT EXISTS idx_sessions_start
                ON sessions(start_time);
            ",
        )
        .map_err(|e| format!("Failed to init telemetry schema: {}", e))
    }

    // ── Recording ──────────────────────────────────────────────

    /// Record a single conversation turn.
    ///
    /// `tools` is a list of tool names invoked during this turn.
    pub fn record_turn(
        &self,
        session_id: &str,
        tokens_in: u64,
        tokens_out: u64,
        cost: f64,
        response_time_ms: u64,
        tools: Vec<String>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        let tools_json = serde_json::to_string(&tools)
            .unwrap_or_else(|_| "[]".to_string());

        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO turns (session_id, timestamp, tokens_in, tokens_out, cost_usd, response_time_ms, tools_used)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session_id,
                now,
                tokens_in as i64,
                tokens_out as i64,
                cost,
                response_time_ms as i64,
                tools_json,
            ],
        )
        .map_err(|e| format!("Failed to record turn: {}", e))?;

        Ok(())
    }

    /// Record a completed session. Updates the session entry with final stats.
    pub fn record_session(
        &self,
        session_id: &str,
        start_time: i64,
        end_time: i64,
        turns: u64,
        total_tokens: u64,
        total_cost: f64,
        tools_used: u64,
        files_changed: u64,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO sessions (session_id, start_time, end_time, turns, total_tokens, total_cost, tools_used, files_changed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(session_id) DO UPDATE SET
                end_time = excluded.end_time,
                turns = excluded.turns,
                total_tokens = excluded.total_tokens,
                total_cost = excluded.total_cost,
                tools_used = excluded.tools_used,
                files_changed = excluded.files_changed",
            params![
                session_id,
                start_time,
                end_time,
                turns as i64,
                total_tokens as i64,
                total_cost,
                tools_used as i64,
                files_changed as i64,
            ],
        )
        .map_err(|e| format!("Failed to record session: {}", e))?;

        Ok(())
    }

    // ── Queries ────────────────────────────────────────────────

    /// Get aggregated usage statistics across all sessions.
    pub fn get_stats(&self) -> Result<UsageStats, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        // Aggregate from turns
        let mut stats = UsageStats::default();

        let mut stmt = conn
            .prepare(
                "SELECT COUNT(*), COALESCE(SUM(tokens_in + tokens_out), 0),
                        COALESCE(SUM(cost_usd), 0.0),
                        COALESCE(AVG(response_time_ms), 0.0),
                        MIN(timestamp), MAX(timestamp)
                 FROM turns",
            )
            .map_err(|e| format!("Query error: {}", e))?;

        let row = stmt
            .query_row([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })
            .map_err(|e| format!("Query error: {}", e))?;

        stats.total_turns = row.0 as u64;
        stats.total_tokens = row.1 as u64;
        stats.total_cost_usd = row.2;
        stats.avg_response_time_ms = row.3;
        stats.first_recorded_at = row.4;
        stats.last_recorded_at = row.5;

        // Count total tool calls (sum of array lengths in tools_used)
        let mut tool_stmt = conn
            .prepare("SELECT tools_used FROM turns")
            .map_err(|e| format!("Query error: {}", e))?;

        let tool_rows = tool_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Query error: {}", e))?;

        let mut tool_counts: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        let mut total_tools = 0u64;

        for row_result in tool_rows {
            if let Ok(tools_json) = row_result {
                if let Ok(tools) = serde_json::from_str::<Vec<String>>(&tools_json) {
                    total_tools += tools.len() as u64;
                    for tool in tools {
                        *tool_counts.entry(tool).or_insert(0) += 1;
                    }
                }
            }
        }
        stats.total_tools_called = total_tools;

        // Count sessions
        let session_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap_or(0);
        stats.sessions_count = session_count as u64;

        // Most used tool
        stats.most_used_tool = tool_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(name, _)| name);

        // Files modified per session
        let files_modified: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(files_changed), 0) FROM sessions",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        stats.files_modified = files_modified as u64;

        Ok(stats)
    }

    /// Get per-tool usage statistics across all turns.
    pub fn get_tool_usage(&self) -> Result<Vec<ToolUsageStat>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn
            .prepare("SELECT tools_used FROM turns")
            .map_err(|e| format!("Query error: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Query error: {}", e))?;

        let mut tool_map: std::collections::HashMap<String, ToolUsageStat> =
            std::collections::HashMap::new();

        for row_result in rows {
            if let Ok(tools_json) = row_result {
                if let Ok(tools) = serde_json::from_str::<Vec<String>>(&tools_json) {
                    for tool in tools {
                        let entry = tool_map
                            .entry(tool.clone())
                            .or_insert_with(|| ToolUsageStat::new(tool));
                        entry.call_count += 1;
                        // We don't track success/error per call here;
                        // this is a simplified count. In a full implementation,
                        // these would come from individual tool result records.
                        entry.success_count += 1;
                    }
                }
            }
        }

        // Also collect tool names from the turns where the tools_used JSON is stored
        let mut result: Vec<ToolUsageStat> = tool_map.into_values().collect();
        result.sort_by(|a, b| b.call_count.cmp(&a.call_count));

        Ok(result)
    }

    /// Get daily aggregated statistics for the last `days` days.
    pub fn get_daily_stats(&self, days: u32) -> Result<Vec<DailyStats>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let cutoff = chrono::Utc::now().timestamp() - (days as i64 * 86400);

        let mut stmt = conn
            .prepare(
                "SELECT date(timestamp, 'unixepoch') as day,
                        COUNT(*) as turn_count,
                        COALESCE(SUM(tokens_in + tokens_out), 0) as total_tokens,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost,
                        COUNT(DISTINCT session_id) as session_count
                 FROM turns
                 WHERE timestamp >= ?1
                 GROUP BY day
                 ORDER BY day ASC",
            )
            .map_err(|e| format!("Query error: {}", e))?;

        let rows = stmt
            .query_map(params![cutoff], |row| {
                Ok(DailyStats {
                    date: row.get(0)?,
                    turns: row.get::<_, i64>(1)? as u64,
                    tokens: row.get::<_, i64>(2)? as u64,
                    cost: row.get(3)?,
                    tools: 0, // we'll fill these in a second pass
                    sessions: row.get::<_, i64>(4)? as u64,
                })
            })
            .map_err(|e| format!("Query error: {}", e))?;

        let mut result: Vec<DailyStats> = Vec::new();
        for row in rows {
            let mut ds = row.map_err(|e| format!("Row error: {}", e))?;

            // Count tools for this day
            let tool_count: i64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(json_array_length(tools_used)), 0)
                     FROM turns
                     WHERE date(timestamp, 'unixepoch') = ?1",
                    params![ds.date],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            ds.tools = tool_count as u64;

            result.push(ds);
        }

        Ok(result)
    }

    /// Get session snapshots, most recent first, up to `limit`.
    pub fn get_sessions(&self, limit: u32) -> Result<Vec<SessionSnapshot>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn
            .prepare(
                "SELECT session_id, start_time, end_time, turns,
                        total_tokens, total_cost, tools_used, files_changed
                 FROM sessions
                 ORDER BY start_time DESC
                 LIMIT ?1",
            )
            .map_err(|e| format!("Query error: {}", e))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(SessionSnapshot {
                    session_id: row.get(0)?,
                    start_time: row.get(1)?,
                    end_time: row.get(2)?,
                    turns: row.get::<_, i64>(3)? as u64,
                    tokens: row.get::<_, i64>(4)? as u64,
                    cost: row.get(5)?,
                    tools_used: row.get::<_, i64>(6)? as u64,
                    files_changed: row.get::<_, i64>(7)? as u64,
                })
            })
            .map_err(|e| format!("Query error: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("Row error: {}", e))?);
        }

        Ok(result)
    }

    /// Get the raw connection for advanced queries (used by TrendAnalyzer).
    pub(crate) fn with_conn<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> Result<R, String>,
    {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;
        f(&conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_collector() -> TelemetryCollector {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_telemetry.db");
        TelemetryCollector::with_path(&path).unwrap()
    }

    #[test]
    fn test_new_collector_and_schema() {
        let collector = test_collector();
        // Should be able to query stats with no data
        let stats = collector.get_stats().unwrap();
        assert_eq!(stats.total_turns, 0);
        assert_eq!(stats.total_tokens, 0);
    }

    #[test]
    fn test_record_turn() {
        let collector = test_collector();
        collector
            .record_turn(
                "session-1",
                100,
                50,
                0.002,
                1200,
                vec!["Bash".to_string(), "FileRead".to_string()],
            )
            .unwrap();

        let stats = collector.get_stats().unwrap();
        assert_eq!(stats.total_turns, 1);
        assert_eq!(stats.total_tokens, 150);
        assert_eq!(stats.total_cost_usd, 0.002);
        assert_eq!(stats.total_tools_called, 2);
    }

    #[test]
    fn test_record_multiple_turns() {
        let collector = test_collector();
        collector
            .record_turn("s1", 100, 50, 0.001, 1000, vec!["Bash".to_string()])
            .unwrap();
        collector
            .record_turn("s1", 200, 100, 0.002, 1500, vec!["FileRead".to_string()])
            .unwrap();
        collector
            .record_turn("s2", 300, 150, 0.003, 2000, vec!["Bash".to_string(), "Grep".to_string()])
            .unwrap();

        let stats = collector.get_stats().unwrap();
        assert_eq!(stats.total_turns, 3);
        assert_eq!(stats.total_tokens, 900);
        assert_eq!(stats.total_tools_called, 4);
    }

    #[test]
    fn test_record_session() {
        let collector = test_collector();
        collector
            .record_session("session-abc", 1700000000, 1700003600, 10, 5000, 0.05, 20, 5)
            .unwrap();

        let sessions = collector.get_sessions(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-abc");
        assert_eq!(sessions[0].turns, 10);
        assert_eq!(sessions[0].tokens, 5000);
        assert_eq!(sessions[0].cost, 0.05);
        assert_eq!(sessions[0].tools_used, 20);
        assert_eq!(sessions[0].files_changed, 5);
    }

    #[test]
    fn test_session_upsert() {
        let collector = test_collector();
        // Insert
        collector
            .record_session("s-1", 1000, 2000, 5, 100, 0.01, 10, 2)
            .unwrap();
        // Update (upsert)
        collector
            .record_session("s-1", 1000, 3000, 10, 200, 0.02, 20, 4)
            .unwrap();

        let sessions = collector.get_sessions(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].turns, 10); // updated
        assert_eq!(sessions[0].tokens, 200); // updated
        assert_eq!(sessions[0].end_time, Some(3000)); // updated
    }

    #[test]
    fn test_get_tool_usage() {
        let collector = test_collector();
        collector
            .record_turn("s1", 10, 5, 0.001, 500, vec!["Bash".to_string()])
            .unwrap();
        collector
            .record_turn("s1", 10, 5, 0.001, 500, vec!["Bash".to_string(), "FileRead".to_string()])
            .unwrap();
        collector
            .record_turn("s2", 10, 5, 0.001, 500, vec!["Grep".to_string()])
            .unwrap();

        let tool_stats = collector.get_tool_usage().unwrap();
        assert_eq!(tool_stats.len(), 3);
        // Bash should be most used
        assert_eq!(tool_stats[0].tool_name, "Bash");
        assert_eq!(tool_stats[0].call_count, 3);
    }

    #[test]
    fn test_get_daily_stats() {
        let collector = test_collector();
        // Record some turns - they'll all be "today"
        collector
            .record_turn("s1", 100, 50, 0.001, 1000, vec!["Bash".to_string()])
            .unwrap();

        let daily = collector.get_daily_stats(30).unwrap();
        // Should have at least today
        assert!(!daily.is_empty());
        let today_stats = daily.last().unwrap();
        let today_str = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(today_stats.date, today_str);
        assert_eq!(today_stats.turns, 1);
        assert_eq!(today_stats.tokens, 150);
        assert_eq!(today_stats.tools, 1);
    }

    #[test]
    fn test_get_sessions_limit() {
        let collector = test_collector();
        for i in 0..5 {
            collector
                .record_session(
                    &format!("sess-{}", i),
                    1000 + i * 100,
                    2000 + i * 100,
                    10,
                    100,
                    0.01,
                    5,
                    1,
                )
                .unwrap();
        }

        let all = collector.get_sessions(10).unwrap();
        assert_eq!(all.len(), 5);

        let limited = collector.get_sessions(2).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_stats_most_used_tool() {
        let collector = test_collector();
        collector
            .record_turn("s1", 10, 5, 0.001, 500, vec!["FileRead".to_string(), "FileRead".to_string()])
            .unwrap();
        collector
            .record_turn("s1", 10, 5, 0.001, 500, vec!["Bash".to_string()])
            .unwrap();
        collector
            .record_turn("s2", 10, 5, 0.001, 500, vec!["FileRead".to_string()])
            .unwrap();

        let stats = collector.get_stats().unwrap();
        // FileRead appears 3 times total (2+1), Bash appears 1 time
        assert_eq!(stats.most_used_tool, Some("FileRead".to_string()));
        assert_eq!(stats.total_tools_called, 4);
    }
}
