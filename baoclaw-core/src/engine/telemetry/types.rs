//! Core data types for the Telemetry & Performance Monitoring system.
//!
//! This module defines the fundamental types used for collecting, aggregating,
//! and analyzing usage statistics and performance data.

use serde::{Deserialize, Serialize};

/// Aggregated usage statistics across all sessions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageStats {
    /// Total number of conversation turns recorded.
    pub total_turns: u64,
    /// Total number of tokens consumed (input + output).
    pub total_tokens: u64,
    /// Total cost in USD accumulated.
    pub total_cost_usd: f64,
    /// Total number of tool invocations.
    pub total_tools_called: u64,
    /// Total number of distinct sessions.
    pub sessions_count: u64,
    /// Total number of files modified (created, edited, deleted).
    pub files_modified: u64,
    /// Average response time in milliseconds.
    pub avg_response_time_ms: f64,
    /// The most frequently used tool name, if any.
    pub most_used_tool: Option<String>,
    /// Earliest recorded turn timestamp (Unix epoch seconds).
    pub first_recorded_at: Option<i64>,
    /// Latest recorded turn timestamp (Unix epoch seconds).
    pub last_recorded_at: Option<i64>,
}

impl Default for UsageStats {
    fn default() -> Self {
        Self {
            total_turns: 0,
            total_tokens: 0,
            total_cost_usd: 0.0,
            total_tools_called: 0,
            sessions_count: 0,
            files_modified: 0,
            avg_response_time_ms: 0.0,
            most_used_tool: None,
            first_recorded_at: None,
            last_recorded_at: None,
        }
    }
}

/// Per-tool usage statistics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolUsageStat {
    /// Name of the tool.
    pub tool_name: String,
    /// Total number of times this tool was called.
    pub call_count: u64,
    /// Number of successful invocations.
    pub success_count: u64,
    /// Number of failed/error invocations.
    pub error_count: u64,
    /// Average duration per invocation in milliseconds.
    pub avg_duration_ms: f64,
}

impl ToolUsageStat {
    /// Create a new stat entry for a tool.
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            call_count: 0,
            success_count: 0,
            error_count: 0,
            avg_duration_ms: 0.0,
        }
    }

    /// Success rate as a fraction (0.0 to 1.0).
    pub fn success_rate(&self) -> f64 {
        if self.call_count == 0 {
            return 1.0;
        }
        self.success_count as f64 / self.call_count as f64
    }
}

/// A snapshot of a single session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Unique session identifier.
    pub session_id: String,
    /// Session start time (Unix epoch seconds).
    pub start_time: i64,
    /// Session end time (Unix epoch seconds).
    pub end_time: Option<i64>,
    /// Number of turns in this session.
    pub turns: u64,
    /// Total tokens used in this session.
    pub tokens: u64,
    /// Total cost in USD for this session.
    pub cost: f64,
    /// Number of tool invocations in this session.
    pub tools_used: u64,
    /// Number of files changed in this session.
    pub files_changed: u64,
}

impl SessionSnapshot {
    /// Duration of the session in seconds.
    pub fn duration_secs(&self) -> Option<i64> {
        self.end_time.map(|end| end.saturating_sub(self.start_time))
    }
}

/// Daily aggregated statistics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyStats {
    /// Date string in YYYY-MM-DD format.
    pub date: String,
    /// Number of turns on this day.
    pub turns: u64,
    /// Tokens consumed on this day.
    pub tokens: u64,
    /// Cost in USD for this day.
    pub cost: f64,
    /// Number of tool invocations on this day.
    pub tools: u64,
    /// Number of sessions active on this day.
    pub sessions: u64,
}

/// Direction of a trend.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    /// Metric is increasing.
    Up,
    /// Metric is decreasing.
    Down,
    /// Metric is relatively unchanged.
    Flat,
}

impl std::fmt::Display for TrendDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Up => write!(f, "↑"),
            Self::Down => write!(f, "↓"),
            Self::Flat => write!(f, "→"),
        }
    }
}

/// A trend analysis result comparing two time periods.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trend {
    /// The metric being measured (e.g. "turns", "tokens", "cost", "tools").
    pub metric: String,
    /// The value for the current period.
    pub current_value: f64,
    /// The value for the previous period.
    pub previous_value: f64,
    /// Percentage change: ((current - previous) / previous) * 100.
    /// None if previous was 0.
    pub change_pct: Option<f64>,
    /// Direction of the trend.
    pub direction: TrendDirection,
}

impl Trend {
    /// Create a new trend analysis result.
    pub fn new(metric: impl Into<String>, current: f64, previous: f64) -> Self {
        let change_pct = if previous == 0.0 {
            // If current is also 0, flat; otherwise up
            None
        } else {
            Some(((current - previous) / previous) * 100.0)
        };

        let direction = match change_pct {
            None if current > 0.0 => TrendDirection::Up,
            None => TrendDirection::Flat,
            Some(pct) if pct > 5.0 => TrendDirection::Up,
            Some(pct) if pct < -5.0 => TrendDirection::Down,
            _ => TrendDirection::Flat,
        };

        Self {
            metric: metric.into(),
            current_value: current,
            previous_value: previous,
            change_pct,
            direction,
        }
    }

    /// The formatted direction arrow.
    pub fn direction_symbol(&self) -> &str {
        match self.direction {
            TrendDirection::Up => "↑",
            TrendDirection::Down => "↓",
            TrendDirection::Flat => "→",
        }
    }

    /// Human-readable summary line.
    pub fn summary(&self) -> String {
        match self.change_pct {
            Some(pct) => format!(
                "{} {:.0} {}% ({:.1} → {:.1})",
                self.direction_symbol(),
                pct.abs(),
                self.metric,
                self.previous_value,
                self.current_value
            ),
            None => format!(
                "{} {} (0 → {:.1})",
                self.direction_symbol(),
                self.metric,
                self.current_value
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_stats_default() {
        let stats = UsageStats::default();
        assert_eq!(stats.total_turns, 0);
        assert_eq!(stats.total_tokens, 0);
        assert_eq!(stats.total_cost_usd, 0.0);
        assert_eq!(stats.total_tools_called, 0);
        assert_eq!(stats.sessions_count, 0);
        assert_eq!(stats.files_modified, 0);
        assert_eq!(stats.avg_response_time_ms, 0.0);
        assert!(stats.most_used_tool.is_none());
        assert!(stats.first_recorded_at.is_none());
        assert!(stats.last_recorded_at.is_none());
    }

    #[test]
    fn test_tool_usage_stat_success_rate() {
        let stat = ToolUsageStat {
            tool_name: "Bash".to_string(),
            call_count: 10,
            success_count: 8,
            error_count: 2,
            avg_duration_ms: 150.0,
        };
        assert!((stat.success_rate() - 0.8).abs() < 0.001);

        let empty = ToolUsageStat::new("Test");
        assert!((empty.success_rate() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_session_snapshot_duration() {
        let snap = SessionSnapshot {
            session_id: "s1".to_string(),
            start_time: 1000,
            end_time: Some(1500),
            turns: 10,
            tokens: 5000,
            cost: 0.05,
            tools_used: 20,
            files_changed: 3,
        };
        assert_eq!(snap.duration_secs(), Some(500));
    }

    #[test]
    fn test_session_snapshot_no_end() {
        let snap = SessionSnapshot {
            session_id: "s1".to_string(),
            start_time: 1000,
            end_time: None,
            turns: 10,
            tokens: 5000,
            cost: 0.05,
            tools_used: 20,
            files_changed: 3,
        };
        assert_eq!(snap.duration_secs(), None);
    }

    #[test]
    fn test_trend_up() {
        let trend = Trend::new("turns", 150.0, 100.0);
        assert_eq!(trend.direction, TrendDirection::Up);
        assert_eq!(trend.change_pct, Some(50.0));
        assert_eq!(trend.direction_symbol(), "↑");
    }

    #[test]
    fn test_trend_down() {
        let trend = Trend::new("cost", 5.0, 10.0);
        assert_eq!(trend.direction, TrendDirection::Down);
        assert_eq!(trend.change_pct, Some(-50.0));
        assert_eq!(trend.direction_symbol(), "↓");
    }

    #[test]
    fn test_trend_flat() {
        let trend = Trend::new("tools", 100.0, 98.0);
        assert_eq!(trend.direction, TrendDirection::Flat);
        assert_eq!(trend.direction_symbol(), "→");
    }

    #[test]
    fn test_trend_from_zero_previous() {
        let trend = Trend::new("tokens", 50.0, 0.0);
        assert_eq!(trend.direction, TrendDirection::Up);
        assert_eq!(trend.change_pct, None);
    }

    #[test]
    fn test_trend_from_zero_both() {
        let trend = Trend::new("cost", 0.0, 0.0);
        assert_eq!(trend.direction, TrendDirection::Flat);
        assert_eq!(trend.change_pct, None);
    }

    #[test]
    fn test_trend_direction_display() {
        assert_eq!(TrendDirection::Up.to_string(), "↑");
        assert_eq!(TrendDirection::Down.to_string(), "↓");
        assert_eq!(TrendDirection::Flat.to_string(), "→");
    }

    #[test]
    fn test_usage_stats_serialization() {
        let stats = UsageStats {
            total_turns: 42,
            total_tokens: 10000,
            total_cost_usd: 1.5,
            total_tools_called: 200,
            sessions_count: 5,
            files_modified: 15,
            avg_response_time_ms: 3200.5,
            most_used_tool: Some("Bash".to_string()),
            first_recorded_at: Some(1700000000),
            last_recorded_at: Some(1700086400),
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: UsageStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_turns, 42);
        assert_eq!(deserialized.most_used_tool, Some("Bash".to_string()));
    }

    #[test]
    fn test_daily_stats_serialization() {
        let stats = DailyStats {
            date: "2026-01-15".to_string(),
            turns: 20,
            tokens: 5000,
            cost: 0.75,
            tools: 80,
            sessions: 2,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: DailyStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.date, "2026-01-15");
        assert_eq!(deserialized.turns, 20);
    }
}
