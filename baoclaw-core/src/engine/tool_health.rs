//! Tool health tracking — learns success/failure rates and auto-degrades failing tools.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Health record for a single tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolHealthRecord {
    pub tool_name: String,
    pub total_calls: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub timeout_count: u64,
    /// Recent failure reasons (last 10).
    pub recent_failures: Vec<String>,
    /// Current health status.
    pub status: ToolStatus,
    /// Consecutive failures (reset on success).
    pub consecutive_failures: u32,
    /// Timestamp of last status change.
    pub last_status_change: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToolStatus {
    /// Tool is healthy — normal operation.
    Healthy,
    /// Tool is degraded — added warnings to prompts.
    Degraded,
    /// Tool is disabled — temporarily removed from available tools.
    Disabled,
}

/// Manages tool health across sessions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolHealthTracker {
    pub records: HashMap<String, ToolHealthRecord>,
    /// Threshold: consecutive failures before degrading.
    pub degrade_threshold: u32,
    /// Threshold: consecutive failures before disabling.
    pub disable_threshold: u32,
    /// Auto-recovery: after N minutes of being degraded, try healthy again.
    pub recovery_minutes: u32,
}

impl Default for ToolHealthTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolHealthTracker {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            degrade_threshold: 3,
            disable_threshold: 6,
            recovery_minutes: 30,
        }
    }

    /// Record a successful tool call.
    pub fn record_success(&mut self, tool_name: &str) {
        let record = self.records.entry(tool_name.to_string())
            .or_insert_with(|| ToolHealthRecord::new(tool_name));
        record.total_calls += 1;
        record.success_count += 1;
        record.consecutive_failures = 0;
        // If was degraded, check if we should recover
        if record.status == ToolStatus::Degraded && record.success_count.is_multiple_of(5) {
            record.status = ToolStatus::Healthy;
            record.last_status_change = chrono::Utc::now().to_rfc3339();
        }
    }

    /// Record a failed tool call.
    pub fn record_failure(&mut self, tool_name: &str, reason: &str) {
        let record = self.records.entry(tool_name.to_string())
            .or_insert_with(|| ToolHealthRecord::new(tool_name));
        record.total_calls += 1;
        record.failure_count += 1;
        record.consecutive_failures += 1;
        record.recent_failures.push(reason.to_string());
        if record.recent_failures.len() > 10 {
            record.recent_failures.drain(0..1);
        }
        // Check thresholds
        if record.consecutive_failures >= self.disable_threshold {
            record.status = ToolStatus::Disabled;
            record.last_status_change = chrono::Utc::now().to_rfc3339();
        } else if record.consecutive_failures >= self.degrade_threshold {
            record.status = ToolStatus::Degraded;
            record.last_status_change = chrono::Utc::now().to_rfc3339();
        }
    }

    /// Record a timeout.
    pub fn record_timeout(&mut self, tool_name: &str) {
        let record = self.records.entry(tool_name.to_string())
            .or_insert_with(|| ToolHealthRecord::new(tool_name));
        record.total_calls += 1;
        record.timeout_count += 1;
        record.consecutive_failures += 1;
        if record.consecutive_failures >= self.disable_threshold {
            record.status = ToolStatus::Disabled;
            record.last_status_change = chrono::Utc::now().to_rfc3339();
        } else if record.consecutive_failures >= self.degrade_threshold {
            record.status = ToolStatus::Degraded;
            record.last_status_change = chrono::Utc::now().to_rfc3339();
        }
    }

    /// Check if a tool is available (not disabled).
    pub fn is_available(&self, tool_name: &str) -> bool {
        self.records.get(tool_name)
            .map(|r| r.status != ToolStatus::Disabled)
            .unwrap_or(true) // unknown tools are available by default
    }

    /// Get warning message for degraded tools (to inject into system prompt).
    pub fn get_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for record in self.records.values() {
            match record.status {
                ToolStatus::Degraded => {
                    let rate = if record.total_calls > 0 {
                        record.failure_count as f64 / record.total_calls as f64 * 100.0
                    } else { 0.0 };
                    warnings.push(format!(
                        "⚠️ Tool '{}' is degraded (failure rate: {:.0}%, {} consecutive failures). Consider using an alternative.",
                        record.tool_name, rate, record.consecutive_failures
                    ));
                }
                ToolStatus::Disabled => {
                    warnings.push(format!(
                        "🚫 Tool '{}' is temporarily disabled due to repeated failures. Last failures: {}",
                        record.tool_name,
                        record.recent_failures.last().unwrap_or(&"unknown".to_string())
                    ));
                }
                ToolStatus::Healthy => {}
            }
        }
        warnings
    }

    /// Get list of currently disabled tool names.
    pub fn disabled_tools(&self) -> Vec<String> {
        self.records.iter()
            .filter(|(_, r)| r.status == ToolStatus::Disabled)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Get list of degraded tool names.
    pub fn degraded_tools(&self) -> Vec<String> {
        self.records.iter()
            .filter(|(_, r)| r.status == ToolStatus::Degraded)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Build a system prompt fragment warning about tool health.
    pub fn build_health_prompt(&self) -> Option<String> {
        let warnings = self.get_warnings();
        if warnings.is_empty() {
            None
        } else {
            Some(format!("\n## Tool Health Warnings\n{}\n", warnings.join("\n")))
        }
    }

    /// Force-enable a disabled tool (manual override).
    pub fn force_enable(&mut self, tool_name: &str) {
        if let Some(record) = self.records.get_mut(tool_name) {
            record.status = ToolStatus::Healthy;
            record.consecutive_failures = 0;
            record.last_status_change = chrono::Utc::now().to_rfc3339();
        }
    }
}

impl ToolHealthRecord {
    fn new(name: &str) -> Self {
        Self {
            tool_name: name.to_string(),
            total_calls: 0,
            success_count: 0,
            failure_count: 0,
            timeout_count: 0,
            recent_failures: Vec::new(),
            status: ToolStatus::Healthy,
            consecutive_failures: 0,
            last_status_change: chrono::Utc::now().to_rfc3339(),
        }
    }
}
