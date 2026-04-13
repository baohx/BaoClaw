use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// A single telemetry event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub event_type: String,
    pub timestamp: String,
    pub data: Value,
}

/// Collects telemetry events locally with opt-in control.
///
/// Telemetry is disabled by default. When enabled, events are
/// accumulated in memory and can be flushed to `~/.baoclaw/telemetry/`.
pub struct TelemetryCollector {
    enabled: bool,
    events: Vec<TelemetryEvent>,
    store_dir: PathBuf,
}

impl TelemetryCollector {
    /// Create a new collector. Telemetry is disabled by default.
    pub fn new() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/tmp".to_string());
        Self {
            enabled: false,
            events: Vec::new(),
            store_dir: PathBuf::from(home).join(".baoclaw").join("telemetry"),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Record a telemetry event. Only records if telemetry is enabled.
    pub fn record(&mut self, event_type: &str, data: Value) {
        if !self.enabled {
            return;
        }
        self.events.push(TelemetryEvent {
            event_type: event_type.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            data,
        });
    }

    /// Get the number of pending events.
    pub fn pending_count(&self) -> usize {
        self.events.len()
    }

    /// Flush pending events to disk at `~/.baoclaw/telemetry/`.
    pub async fn flush(&self) -> Result<(), std::io::Error> {
        if self.events.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.store_dir)?;
        let filename = format!(
            "events-{}.jsonl",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        let path = self.store_dir.join(filename);
        let mut content = String::new();
        for event in &self.events {
            if let Ok(line) = serde_json::to_string(event) {
                content.push_str(&line);
                content.push('\n');
            }
        }
        std::fs::write(&path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_collector_is_disabled() {
        let collector = TelemetryCollector::new();
        assert!(!collector.is_enabled());
        assert_eq!(collector.pending_count(), 0);
    }

    #[test]
    fn test_enable_disable() {
        let mut collector = TelemetryCollector::new();
        assert!(!collector.is_enabled());

        collector.enable();
        assert!(collector.is_enabled());

        collector.disable();
        assert!(!collector.is_enabled());
    }

    #[test]
    fn test_record_when_disabled_does_nothing() {
        let mut collector = TelemetryCollector::new();
        collector.record("test_event", serde_json::json!({"key": "value"}));
        assert_eq!(collector.pending_count(), 0);
    }

    #[test]
    fn test_record_when_enabled() {
        let mut collector = TelemetryCollector::new();
        collector.enable();
        collector.record("query", serde_json::json!({"tokens": 100}));
        collector.record("tool_use", serde_json::json!({"tool": "bash"}));
        assert_eq!(collector.pending_count(), 2);
    }

    #[test]
    fn test_record_preserves_event_data() {
        let mut collector = TelemetryCollector::new();
        collector.enable();
        collector.record("test", serde_json::json!({"foo": "bar"}));

        let event = &collector.events[0];
        assert_eq!(event.event_type, "test");
        assert_eq!(event.data["foo"], "bar");
        assert!(!event.timestamp.is_empty());
    }

    #[tokio::test]
    async fn test_flush_empty_is_ok() {
        let collector = TelemetryCollector::new();
        let result = collector.flush().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_flush_writes_to_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let mut collector = TelemetryCollector::new();
        collector.store_dir = tmp.path().to_path_buf();
        collector.enable();
        collector.record("test", serde_json::json!({"x": 1}));

        let result = collector.flush().await;
        assert!(result.is_ok());

        // Verify a file was written
        let entries: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_telemetry_event_serialization() {
        let event = TelemetryEvent {
            event_type: "query".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            data: serde_json::json!({"model": "claude-3"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        let rt: TelemetryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.event_type, "query");
        assert_eq!(rt.timestamp, "2024-01-15T10:30:00Z");
        assert_eq!(rt.data["model"], "claude-3");
    }
}
