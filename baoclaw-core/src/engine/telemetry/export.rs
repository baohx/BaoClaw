//! Data export for telemetry data.
//!
//! `TelemetryExporter` can export collected telemetry data to
//! JSON, CSV, and Markdown summary formats.

use super::collector::TelemetryCollector;
use super::types::{DailyStats, SessionSnapshot, ToolUsageStat, UsageStats};

/// Exports telemetry data to various formats.
pub struct TelemetryExporter {
    collector: TelemetryCollector,
}

impl TelemetryExporter {
    /// Create a new exporter bound to the given collector.
    pub fn new(collector: TelemetryCollector) -> Self {
        Self { collector }
    }

    /// Export all telemetry data as JSON.
    ///
    /// If `path` is provided, writes to that file; otherwise returns
    /// the JSON string without writing to disk.
    pub fn export_json(&self, path: Option<&str>) -> Result<String, String> {
        let stats = self.collector.get_stats()?;
        let tool_usage = self.collector.get_tool_usage()?;
        let daily_stats = self.collector.get_daily_stats(365)?;
        let sessions = self.collector.get_sessions(1000)?;

        let export = ExportPayload {
            exported_at: chrono::Utc::now().to_rfc3339(),
            stats,
            tool_usage,
            daily_stats,
            sessions,
        };

        let json =
            serde_json::to_string_pretty(&export).map_err(|e| format!("Serialization error: {}", e))?;

        if let Some(file_path) = path {
            std::fs::write(file_path, &json)
                .map_err(|e| format!("Failed to write JSON: {}", e))?;
        }

        Ok(json)
    }

    /// Export telemetry data as CSV (returns CSV string).
    ///
    /// If `path` is provided, writes to that file; otherwise returns
    /// the CSV string. The CSV contains daily stats and tool usage.
    pub fn export_csv(&self, path: Option<&str>) -> Result<String, String> {
        let daily_stats = self.collector.get_daily_stats(365)?;
        let tool_usage = self.collector.get_tool_usage()?;
        let sessions = self.collector.get_sessions(1000)?;

        let mut csv = String::new();

        // Section: Daily Stats
        csv.push_str("# Daily Statistics\n");
        csv.push_str("date,turns,tokens,cost,tools,sessions\n");
        for day in &daily_stats {
            csv.push_str(&format!(
                "{},{},{},{:.6},{},{}\n",
                day.date, day.turns, day.tokens, day.cost, day.tools, day.sessions
            ));
        }

        csv.push('\n');

        // Section: Tool Usage
        csv.push_str("# Tool Usage\n");
        csv.push_str("tool_name,call_count,success_count,error_count,avg_duration_ms\n");
        for tool in &tool_usage {
            csv.push_str(&format!(
                "{},{},{},{},{:.2}\n",
                tool.tool_name,
                tool.call_count,
                tool.success_count,
                tool.error_count,
                tool.avg_duration_ms
            ));
        }

        csv.push('\n');

        // Section: Sessions
        csv.push_str("# Sessions\n");
        csv.push_str("session_id,start_time,end_time,turns,tokens,cost,tools_used,files_changed\n");
        for session in &sessions {
            let end_time = session
                .end_time
                .map(|e| e.to_string())
                .unwrap_or_default();
            csv.push_str(&format!(
                "{},{},{},{},{},{:.6},{},{}\n",
                session.session_id,
                session.start_time,
                end_time,
                session.turns,
                session.tokens,
                session.cost,
                session.tools_used,
                session.files_changed
            ));
        }

        if let Some(file_path) = path {
            std::fs::write(file_path, &csv)
                .map_err(|e| format!("Failed to write CSV: {}", e))?;
        }

        Ok(csv)
    }

    /// Generate a Markdown summary of current usage statistics.
    pub fn export_summary(&self) -> Result<String, String> {
        let stats = self.collector.get_stats()?;
        let tool_usage = self.collector.get_tool_usage()?;
        let daily_stats = self.collector.get_daily_stats(7)?;

        let mut md = String::new();

        md.push_str("# 📊 BaoClaw Telemetry Summary\n\n");

        // General stats
        md.push_str("## Overview\n\n");
        md.push_str(&format!("| Metric | Value |\n"));
        md.push_str(&format!("|--------|-------|\n"));
        md.push_str(&format!("| Total Turns | {} |\n", stats.total_turns));
        md.push_str(&format!(
            "| Total Tokens | {} |\n",
            format_number(stats.total_tokens as f64)
        ));
        md.push_str(&format!(
            "| Total Cost (USD) | ${:.4} |\n",
            stats.total_cost_usd
        ));
        md.push_str(&format!(
            "| Total Tools Called | {} |\n",
            stats.total_tools_called
        ));
        md.push_str(&format!(
            "| Sessions Count | {} |\n",
            stats.sessions_count
        ));
        md.push_str(&format!(
            "| Files Modified | {} |\n",
            stats.files_modified
        ));
        md.push_str(&format!(
            "| Avg Response Time | {:.0} ms |\n",
            stats.avg_response_time_ms
        ));
        if let Some(ref tool) = stats.most_used_tool {
            md.push_str(&format!("| Most Used Tool | `{}` |\n", tool));
        }
        if let Some(first) = stats.first_recorded_at {
            let ts = format_unix(first);
            md.push_str(&format!("| First Recorded | {} |\n", ts));
        }
        if let Some(last) = stats.last_recorded_at {
            let ts = format_unix(last);
            md.push_str(&format!("| Last Recorded | {} |\n", ts));
        }

        // Tool usage breakdown
        if !tool_usage.is_empty() {
            md.push_str("\n## Tool Usage\n\n");
            md.push_str("| Tool | Calls | Success Rate |\n");
            md.push_str("|------|-------|-------------|\n");
            for tool in tool_usage.iter().take(10) {
                md.push_str(&format!(
                    "| `{}` | {} | {:.0}% |\n",
                    tool.tool_name,
                    tool.call_count,
                    tool.success_rate() * 100.0
                ));
            }
            if tool_usage.len() > 10 {
                md.push_str(&format!(
                    "| ... | ... | ({} more tools) |\n",
                    tool_usage.len() - 10
                ));
            }
        }

        // Daily trend (last 7 days)
        if !daily_stats.is_empty() {
            md.push_str("\n## Last 7 Days\n\n");
            md.push_str("| Date | Turns | Tokens | Cost | Tools |\n");
            md.push_str("|------|-------|--------|------|-------|\n");
            for day in &daily_stats {
                md.push_str(&format!(
                    "| {} | {} | {} | ${:.4} | {} |\n",
                    day.date, day.turns, day.tokens, day.cost, day.tools
                ));
            }

            // Simple bar chart
            if daily_stats.len() > 1 {
                md.push_str("\n### Daily Turn Volume\n\n```\n");
                let max_turns = daily_stats
                    .iter()
                    .map(|d| d.turns)
                    .max()
                    .unwrap_or(1)
                    .max(1);
                for day in &daily_stats {
                    let bar_len = (day.turns as f64 / max_turns as f64 * 40.0) as usize;
                    let bar = "█".repeat(bar_len);
                    let day_short = if day.date.len() >= 10 {
                        &day.date[5..10]
                    } else {
                        &day.date
                    };
                    md.push_str(&format!(
                        "{} ▏{} {} turns\n",
                        day_short, bar, day.turns
                    ));
                }
                md.push_str("```\n");
            }
        }

        md.push_str(&format!(
            "\n---\n*Generated at {}*\n",
            chrono::Utc::now().to_rfc3339()
        ));

        Ok(md)
    }
}

/// Payload for JSON export.
#[derive(serde::Serialize, serde::Deserialize)]
struct ExportPayload {
    exported_at: String,
    stats: UsageStats,
    tool_usage: Vec<ToolUsageStat>,
    daily_stats: Vec<DailyStats>,
    sessions: Vec<SessionSnapshot>,
}

/// Format a number with K/M suffixes.
fn format_number(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}K", n / 1_000.0)
    } else {
        format!("{:.0}", n)
    }
}

/// Format a Unix timestamp to a readable string.
fn format_unix(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_exporter() -> (TelemetryExporter, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_export.db");
        let collector = TelemetryCollector::with_path(&path).unwrap();

        // Seed some data
        collector
            .record_session("sess-1", 1700000000, 1700003600, 5, 1000, 0.01, 10, 2)
            .unwrap();
        collector
            .record_session("sess-2", 1700086400, 1700090000, 8, 1600, 0.02, 15, 3)
            .unwrap();

        (TelemetryExporter::new(collector), dir)
    }

    #[test]
    fn test_export_json_no_path() {
        let (exporter, _dir) = setup_exporter();
        let json = exporter.export_json(None).unwrap();
        assert!(json.contains("sess-1"));
        assert!(json.contains("sess-2"));
        assert!(json.contains("\"total_turns\""));
    }

    #[test]
    fn test_export_json_with_path() {
        let (exporter, _dir) = setup_exporter();
        let dir = tempdir().unwrap();
        let out_path = dir.path().join("export.json");
        let _json = exporter
            .export_json(Some(out_path.to_str().unwrap()))
            .unwrap();
        assert!(std::path::Path::new(out_path.to_str().unwrap()).exists());
    }

    #[test]
    fn test_export_csv_no_path() {
        let (exporter, _dir) = setup_exporter();
        let csv = exporter.export_csv(None).unwrap();
        assert!(csv.contains("Daily Statistics"));
        assert!(csv.contains("Tool Usage"));
        assert!(csv.contains("sess-1"));
        assert!(csv.contains("sess-2"));
    }

    #[test]
    fn test_export_csv_with_path() {
        let (exporter, _dir) = setup_exporter();
        let dir = tempdir().unwrap();
        let out_path = dir.path().join("export.csv");
        let _csv = exporter
            .export_csv(Some(out_path.to_str().unwrap()))
            .unwrap();
        assert!(std::path::Path::new(out_path.to_str().unwrap()).exists());
    }

    #[test]
    fn test_export_summary() {
        let (exporter, _dir) = setup_exporter();
        let summary = exporter.export_summary().unwrap();
        assert!(summary.contains("BaoClaw Telemetry Summary"));
        assert!(summary.contains("## Overview"));
        assert!(summary.contains("Total Turns"));
        assert!(summary.contains("Overview"));
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(5.0), "5");
        assert_eq!(format_number(1500.0), "1.5K");
        assert_eq!(format_number(2_500_000.0), "2.5M");
    }
}
