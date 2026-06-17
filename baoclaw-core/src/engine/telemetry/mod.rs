//! Telemetry & Performance Monitoring.
//!
//! This module provides usage statistics collection, trend analysis,
//! and data export capabilities for BaoClaw.
//!
//! # Architecture
//!
//! - `types` - Core data types: UsageStats, ToolUsageStat, SessionSnapshot, DailyStats, Trend
//! - `collector` - SQLite-backed data collection for turns and sessions
//! - `trends` - Trend analysis comparing time periods and forecasting
//! - `export` - Export to JSON, CSV, and Markdown summary
//!
//! # Example
//!
//! ```rust,ignore
//! use baoclaw_core::engine::telemetry::{
//!     TelemetryCollector, TrendAnalyzer, TelemetryExporter,
//!     UsageStats, ToolUsageStat, SessionSnapshot, DailyStats, Trend,
//! };
//!
//! // Create a collector
//! let collector = TelemetryCollector::new().unwrap();
//!
//! // Record a turn
//! collector.record_turn("session-1", 500, 200, 0.005, 1200,
//!     vec!["Bash".to_string(), "FileRead".to_string()]).unwrap();
//!
//! // Get aggregated stats
//! let stats = collector.get_stats().unwrap();
//! println!("Total turns: {}", stats.total_turns);
//!
//! // Analyze trends
//! let analyzer = TrendAnalyzer::new();
//! let trend = analyzer.analyze("turns", 7).unwrap();
//! println!("Trend: {}", trend.summary());
//!
//! // Export data
//! let exporter = TelemetryExporter::new(collector);
//! let json = exporter.export_json(Some("stats.json")).unwrap();
//! let summary = exporter.export_summary().unwrap();
//! ```

pub mod collector;
pub mod export;
pub mod trends;
pub mod types;

// Re-export main types for convenience
pub use collector::TelemetryCollector;
pub use export::TelemetryExporter;
pub use trends::TrendAnalyzer;
pub use types::{DailyStats, SessionSnapshot, ToolUsageStat, Trend, TrendDirection, UsageStats};
