//! Shared state for inter-agent communication.
//!
//! This module implements thread-safe shared state for sub-agent teams:
//! - Key-value store with concurrent access
//! - Progress broadcast mechanism
//! - Result merging strategies
//!
//! # Example
//!
//! ```rust,ignore
//! use baoclaw_core::engine::team::shared_state::SharedStateManager;
//!
//! let manager = SharedStateManager::new();
//!
//! // Key-value operations
//! manager.set("config", serde_json::json!({"key": "value"}));
//! let value = manager.get("config");
//!
//! // Progress broadcasting
//! let mut rx = manager.subscribe_progress();
//! manager.broadcast_progress("agent-1", "Working...", 0.5);
//!
//! // Result merging
//! let results = manager.merge_results(merge_strategy);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Default capacity for the progress broadcast channel.
const DEFAULT_BROADCAST_CAPACITY: usize = 256;

/// A progress event broadcast to subscribers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressEvent {
    /// ID of the agent reporting.
    pub agent_id: String,
    /// Progress message.
    pub message: String,
    /// Progress as a fraction (0.0 to 1.0).
    pub progress: f64,
    /// Timestamp of the report (ISO 8601).
    pub timestamp: String,
    /// Optional additional data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ProgressEvent {
    /// Create a new progress event.
    pub fn new(agent_id: impl Into<String>, message: impl Into<String>, progress: f64) -> Self {
        Self {
            agent_id: agent_id.into(),
            message: message.into(),
            progress: progress.clamp(0.0, 1.0),
            timestamp: chrono::Utc::now().to_rfc3339(),
            data: None,
        }
    }

    /// Add additional data to the event.
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// An agent result ready for merging.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResultForMerge {
    /// ID of the agent.
    pub agent_id: String,
    /// Result text.
    pub text: String,
    /// Whether the agent succeeded.
    pub success: bool,
    /// Token usage.
    pub tokens: u64,
    /// Cost in USD.
    pub cost_usd: f64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Additional metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Strategy for merging agent results.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    /// Concatenate all results with a separator.
    Concat,
    /// Collect results into a JSON array.
    JsonArray,
    /// Summarize results (combine key findings).
    Summarize,
    /// Take only the first result.
    FirstOnly,
    /// Take only the last result.
    LastOnly,
    /// Filter by success (only include successful results).
    SuccessOnly,
    /// Custom merge using a provided function name.
    Custom(String),
}

impl Default for MergeStrategy {
    fn default() -> Self {
        Self::Concat
    }
}

impl std::fmt::Display for MergeStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Concat => write!(f, "concat"),
            Self::JsonArray => write!(f, "json_array"),
            Self::Summarize => write!(f, "summarize"),
            Self::FirstOnly => write!(f, "first_only"),
            Self::LastOnly => write!(f, "last_only"),
            Self::SuccessOnly => write!(f, "success_only"),
            Self::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}

/// Merged results from all agents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergedResults {
    /// Merged text output.
    pub text: String,
    /// Strategy used for merging.
    pub strategy: MergeStrategy,
    /// Number of results merged.
    pub count: usize,
    /// Total tokens across all agents.
    pub total_tokens: u64,
    /// Total cost across all agents.
    pub total_cost_usd: f64,
    /// Total duration across all agents.
    pub total_duration_ms: u64,
    /// Number of successful agents.
    pub success_count: usize,
    /// Number of failed agents.
    pub failure_count: usize,
    /// Individual results for reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub individual_results: Vec<AgentResultForMerge>,
}

/// Manager for shared state with thread-safe access and broadcasting.
pub struct SharedStateManager {
    /// Key-value store for shared data.
    data: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    /// Progress broadcast sender.
    progress_tx: broadcast::Sender<ProgressEvent>,
    /// Agent results for merging.
    results: Arc<RwLock<Vec<AgentResultForMerge>>>,
    /// Metrics tracking.
    metrics: Arc<RwLock<SharedMetrics>>,
}

/// Metrics for the shared state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SharedMetrics {
    /// Total number of key-value operations.
    pub kv_operations: u64,
    /// Total number of progress broadcasts.
    pub progress_broadcasts: u64,
    /// Total number of results stored.
    pub results_stored: u64,
    /// Peak number of keys in the store.
    pub peak_key_count: usize,
    /// Number of active subscribers.
    pub subscriber_count: usize,
}

impl SharedStateManager {
    /// Create a new shared state manager.
    pub fn new() -> Self {
        let (progress_tx, _) = broadcast::channel(DEFAULT_BROADCAST_CAPACITY);
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            progress_tx,
            results: Arc::new(RwLock::new(Vec::new())),
            metrics: Arc::new(RwLock::new(SharedMetrics::default())),
        }
    }

    /// Create with a custom broadcast capacity.
    pub fn with_broadcast_capacity(capacity: usize) -> Self {
        let (progress_tx, _) = broadcast::channel(capacity);
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            progress_tx,
            results: Arc::new(RwLock::new(Vec::new())),
            metrics: Arc::new(RwLock::new(SharedMetrics::default())),
        }
    }

    // =========================================================================
    // Key-Value Store Operations
    // =========================================================================

    /// Set a value in the shared state.
    pub async fn set(&self, key: impl Into<String>, value: serde_json::Value) {
        let key = key.into();
        let mut data = self.data.write().await;
        data.insert(key.clone(), value);
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.kv_operations += 1;
        if data.len() > metrics.peak_key_count {
            metrics.peak_key_count = data.len();
        }
    }

    /// Get a value from the shared state.
    pub async fn get(&self, key: &str) -> Option<serde_json::Value> {
        let data = self.data.read().await;
        let value = data.get(key).cloned();
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.kv_operations += 1;
        
        value
    }

    /// Check if a key exists.
    pub async fn contains(&self, key: &str) -> bool {
        let data = self.data.read().await;
        let exists = data.contains_key(key);
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.kv_operations += 1;
        
        exists
    }

    /// Remove a key from the shared state.
    pub async fn remove(&self, key: &str) -> Option<serde_json::Value> {
        let mut data = self.data.write().await;
        let value = data.remove(key);
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.kv_operations += 1;
        
        value
    }

    /// Get all keys.
    pub async fn keys(&self) -> Vec<String> {
        let data = self.data.read().await;
        data.keys().cloned().collect()
    }

    /// Get the number of keys.
    pub async fn len(&self) -> usize {
        let data = self.data.read().await;
        data.len()
    }

    /// Check if the store is empty.
    pub async fn is_empty(&self) -> bool {
        let data = self.data.read().await;
        data.is_empty()
    }

    /// Clear all data.
    pub async fn clear(&self) {
        let mut data = self.data.write().await;
        data.clear();
    }

    /// Get multiple values by keys.
    pub async fn get_many(&self, keys: &[&str]) -> HashMap<String, serde_json::Value> {
        let data = self.data.read().await;
        keys.iter()
            .filter_map(|k| data.get(*k).map(|v| (k.to_string(), v.clone())))
            .collect()
    }

    /// Set multiple values at once.
    pub async fn set_many(&self, pairs: HashMap<String, serde_json::Value>) {
        let count = pairs.len();
        let mut data = self.data.write().await;
        for (key, value) in pairs {
            data.insert(key, value);
        }
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.kv_operations += count as u64;
        if data.len() > metrics.peak_key_count {
            metrics.peak_key_count = data.len();
        }
    }

    /// Atomically update a value using a closure.
    pub async fn update<F>(&self, key: &str, f: F) -> Option<serde_json::Value>
    where
        F: FnOnce(Option<&serde_json::Value>) -> serde_json::Value,
    {
        let mut data = self.data.write().await;
        let current = data.get(key);
        let new_value = f(current);
        data.insert(key.to_string(), new_value.clone());
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.kv_operations += 1;
        
        Some(new_value)
    }

    /// Increment a numeric value.
    pub async fn increment(&self, key: &str, amount: f64) -> f64 {
        let mut data = self.data.write().await;
        let current = data
            .get(key)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let new_value = current + amount;
        data.insert(key.to_string(), serde_json::json!(new_value));
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.kv_operations += 1;
        
        new_value
    }

    // =========================================================================
    // Progress Broadcasting
    // =========================================================================

    /// Subscribe to progress events.
    pub fn subscribe_progress(&self) -> broadcast::Receiver<ProgressEvent> {
        self.progress_tx.subscribe()
    }

    /// Broadcast a progress event.
    pub async fn broadcast_progress(
        &self,
        agent_id: impl Into<String>,
        message: impl Into<String>,
        progress: f64,
    ) {
        let event = ProgressEvent::new(agent_id, message, progress);
        // Ignore send errors (no subscribers is OK)
        let _ = self.progress_tx.send(event);
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.progress_broadcasts += 1;
    }

    /// Broadcast a progress event with additional data.
    pub async fn broadcast_progress_with_data(
        &self,
        agent_id: impl Into<String>,
        message: impl Into<String>,
        progress: f64,
        data: serde_json::Value,
    ) {
        let event = ProgressEvent::new(agent_id, message, progress).with_data(data);
        let _ = self.progress_tx.send(event);
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.progress_broadcasts += 1;
    }

    /// Get the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.progress_tx.receiver_count()
    }

    // =========================================================================
    // Result Storage and Merging
    // =========================================================================

    /// Store an agent result.
    pub async fn store_result(&self, result: AgentResultForMerge) {
        let mut results = self.results.write().await;
        results.push(result);
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.results_stored += 1;
    }

    /// Store multiple results.
    pub async fn store_results(&self, new_results: Vec<AgentResultForMerge>) {
        let mut results = self.results.write().await;
        let count = new_results.len();
        results.extend(new_results);
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.results_stored += count as u64;
    }

    /// Get all stored results.
    pub async fn get_results(&self) -> Vec<AgentResultForMerge> {
        let results = self.results.read().await;
        results.clone()
    }

    /// Clear all stored results.
    pub async fn clear_results(&self) {
        let mut results = self.results.write().await;
        results.clear();
    }

    /// Merge results using the specified strategy.
    pub async fn merge_results(&self, strategy: MergeStrategy) -> MergedResults {
        let results = self.results.read().await;
        self.merge_with_strategy(&results, strategy).await
    }

    /// Merge with a specific strategy.
    async fn merge_with_strategy(
        &self,
        results: &[AgentResultForMerge],
        strategy: MergeStrategy,
    ) -> MergedResults {
        let text = match &strategy {
            MergeStrategy::Concat => self.merge_concat(results),
            MergeStrategy::JsonArray => self.merge_json_array(results),
            MergeStrategy::Summarize => self.merge_summarize(results),
            MergeStrategy::FirstOnly => self.merge_first(results),
            MergeStrategy::LastOnly => self.merge_last(results),
            MergeStrategy::SuccessOnly => self.merge_success_only(results),
            MergeStrategy::Custom(name) => self.merge_custom(results, name),
        };

        let total_tokens: u64 = results.iter().map(|r| r.tokens).sum();
        let total_cost_usd: f64 = results.iter().map(|r| r.cost_usd).sum();
        let total_duration_ms: u64 = results.iter().map(|r| r.duration_ms).sum();
        let success_count = results.iter().filter(|r| r.success).count();
        let failure_count = results.len() - success_count;

        MergedResults {
            text,
            strategy,
            count: results.len(),
            total_tokens,
            total_cost_usd,
            total_duration_ms,
            success_count,
            failure_count,
            individual_results: results.to_vec(),
        }
    }

    /// Concatenate results with separator.
    fn merge_concat(&self, results: &[AgentResultForMerge]) -> String {
        results
            .iter()
            .map(|r| format!("[{}] {}", r.agent_id, r.text))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    /// Merge as JSON array.
    fn merge_json_array(&self, results: &[AgentResultForMerge]) -> String {
        let json_results: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "agent_id": r.agent_id,
                    "text": r.text,
                    "success": r.success,
                    "tokens": r.tokens,
                    "cost_usd": r.cost_usd,
                    "duration_ms": r.duration_ms,
                })
            })
            .collect();
        serde_json::to_string_pretty(&json_results).unwrap_or_else(|_| "[]".to_string())
    }

    /// Summarize results (combine key findings).
    fn merge_summarize(&self, results: &[AgentResultForMerge]) -> String {
        let successful: Vec<_> = results.iter().filter(|r| r.success).collect();
        let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();

        let mut summary = String::new();
        summary.push_str(&format!("# Team Results Summary\n\n"));
        summary.push_str(&format!("Total agents: {}\n", results.len()));
        summary.push_str(&format!("Successful: {}\n", successful.len()));
        summary.push_str(&format!("Failed: {}\n", failed.len()));
        summary.push_str(&format!(
            "Total tokens: {}\n",
            results.iter().map(|r| r.tokens).sum::<u64>()
        ));
        summary.push_str(&format!(
            "Total cost: ${:.4}\n\n",
            results.iter().map(|r| r.cost_usd).sum::<f64>()
        ));

        if !successful.is_empty() {
            summary.push_str("## Successful Results\n\n");
            for r in &successful {
                summary.push_str(&format!("### Agent: {}\n", r.agent_id));
                summary.push_str(&format!("{}\n\n", r.text));
            }
        }

        if !failed.is_empty() {
            summary.push_str("## Failed Agents\n\n");
            for r in &failed {
                summary.push_str(&format!("- {}: {}\n", r.agent_id, r.text));
            }
        }

        summary
    }

    /// Take only the first result.
    fn merge_first(&self, results: &[AgentResultForMerge]) -> String {
        results
            .first()
            .map(|r| r.text.clone())
            .unwrap_or_default()
    }

    /// Take only the last result.
    fn merge_last(&self, results: &[AgentResultForMerge]) -> String {
        results
            .last()
            .map(|r| r.text.clone())
            .unwrap_or_default()
    }

    /// Filter by success.
    fn merge_success_only(&self, results: &[AgentResultForMerge]) -> String {
        results
            .iter()
            .filter(|r| r.success)
            .map(|r| format!("[{}] {}", r.agent_id, r.text))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    /// Custom merge (placeholder for custom handlers).
    fn merge_custom(&self, results: &[AgentResultForMerge], _handler: &str) -> String {
        // Custom handlers would be registered and called by name
        // For now, default to concat
        self.merge_concat(results)
    }

    // =========================================================================
    // Metrics
    // =========================================================================

    /// Get current metrics.
    pub async fn get_metrics(&self) -> SharedMetrics {
        let mut metrics = self.metrics.write().await;
        metrics.subscriber_count = self.subscriber_count();
        metrics.clone()
    }

    /// Export all shared state as JSON.
    pub async fn export(&self) -> serde_json::Value {
        let data = self.data.read().await;
        let results = self.results.read().await;
        let metrics = self.get_metrics().await;

        serde_json::json!({
            "data": data.clone(),
            "results": results.clone(),
            "metrics": metrics,
        })
    }

    /// Import shared state from JSON.
    pub async fn import(&self, json: serde_json::Value) -> Result<(), String> {
        if let Some(data) = json.get("data").and_then(|d| d.as_object()) {
            let mut store = self.data.write().await;
            store.clear();
            for (key, value) in data {
                store.insert(key.clone(), value.clone());
            }
        }

        if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
            let mut store = self.results.write().await;
            store.clear();
            for result in results {
                if let Ok(parsed) = serde_json::from_value(result.clone()) {
                    store.push(parsed);
                }
            }
        }

        Ok(())
    }
}

impl Default for SharedStateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SharedStateManager {
    fn clone(&self) -> Self {
        Self {
            data: Arc::clone(&self.data),
            progress_tx: self.progress_tx.clone(),
            results: Arc::clone(&self.results),
            metrics: Arc::clone(&self.metrics),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kv_operations() {
        let manager = SharedStateManager::new();

        // Set and get
        manager.set("key1", serde_json::json!("value1")).await;
        let value = manager.get("key1").await;
        assert_eq!(value, Some(serde_json::json!("value1")));

        // Contains
        assert!(manager.contains("key1").await);
        assert!(!manager.contains("key2").await);

        // Remove
        let removed = manager.remove("key1").await;
        assert_eq!(removed, Some(serde_json::json!("value1")));
        assert!(!manager.contains("key1").await);
    }

    #[tokio::test]
    async fn test_kv_many_operations() {
        let manager = SharedStateManager::new();

        let mut pairs = HashMap::new();
        pairs.insert("a".to_string(), serde_json::json!(1));
        pairs.insert("b".to_string(), serde_json::json!(2));
        pairs.insert("c".to_string(), serde_json::json!(3));

        manager.set_many(pairs).await;
        assert_eq!(manager.len().await, 3);

        let values = manager.get_many(&["a", "b", "c"]).await;
        assert_eq!(values.len(), 3);
    }

    #[tokio::test]
    async fn test_increment() {
        let manager = SharedStateManager::new();

        let v1 = manager.increment("counter", 1.0).await;
        assert_eq!(v1, 1.0);

        let v2 = manager.increment("counter", 2.5).await;
        assert_eq!(v2, 3.5);
    }

    #[tokio::test]
    async fn test_progress_broadcast() {
        let manager = SharedStateManager::new();
        let mut rx = manager.subscribe_progress();

        manager.broadcast_progress("agent-1", "Starting", 0.0).await;
        manager.broadcast_progress("agent-1", "Halfway", 0.5).await;
        manager.broadcast_progress("agent-1", "Done", 1.0).await;

        let event1 = rx.try_recv().expect("Should receive event");
        assert_eq!(event1.agent_id, "agent-1");
        assert_eq!(event1.progress, 0.0);

        let event2 = rx.try_recv().expect("Should receive event");
        assert_eq!(event2.progress, 0.5);

        let event3 = rx.try_recv().expect("Should receive event");
        assert_eq!(event3.progress, 1.0);
    }

    #[tokio::test]
    async fn test_result_storage() {
        let manager = SharedStateManager::new();

        let result1 = AgentResultForMerge {
            agent_id: "agent-1".to_string(),
            text: "Result 1".to_string(),
            success: true,
            tokens: 100,
            cost_usd: 0.01,
            duration_ms: 1000,
            metadata: HashMap::new(),
        };

        let result2 = AgentResultForMerge {
            agent_id: "agent-2".to_string(),
            text: "Result 2".to_string(),
            success: true,
            tokens: 200,
            cost_usd: 0.02,
            duration_ms: 2000,
            metadata: HashMap::new(),
        };

        manager.store_result(result1).await;
        manager.store_result(result2).await;

        let results = manager.get_results().await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_merge_concat() {
        let manager = SharedStateManager::new();

        manager.store_result(AgentResultForMerge {
            agent_id: "a".to_string(),
            text: "First".to_string(),
            success: true,
            tokens: 10,
            cost_usd: 0.01,
            duration_ms: 100,
            metadata: HashMap::new(),
        }).await;

        manager.store_result(AgentResultForMerge {
            agent_id: "b".to_string(),
            text: "Second".to_string(),
            success: true,
            tokens: 20,
            cost_usd: 0.02,
            duration_ms: 200,
            metadata: HashMap::new(),
        }).await;

        let merged = manager.merge_results(MergeStrategy::Concat).await;
        assert!(merged.text.contains("[a] First"));
        assert!(merged.text.contains("[b] Second"));
        assert_eq!(merged.count, 2);
        assert_eq!(merged.total_tokens, 30);
    }

    #[tokio::test]
    async fn test_merge_json_array() {
        let manager = SharedStateManager::new();

        manager.store_result(AgentResultForMerge {
            agent_id: "a".to_string(),
            text: "Result".to_string(),
            success: true,
            tokens: 10,
            cost_usd: 0.01,
            duration_ms: 100,
            metadata: HashMap::new(),
        }).await;

        let merged = manager.merge_results(MergeStrategy::JsonArray).await;
        assert!(merged.text.starts_with("["));
        assert!(merged.text.contains("\"agent_id\": \"a\""));
    }

    #[tokio::test]
    async fn test_merge_first_last() {
        let manager = SharedStateManager::new();

        manager.store_result(AgentResultForMerge {
            agent_id: "first".to_string(),
            text: "First result".to_string(),
            success: true,
            tokens: 10,
            cost_usd: 0.01,
            duration_ms: 100,
            metadata: HashMap::new(),
        }).await;

        manager.store_result(AgentResultForMerge {
            agent_id: "last".to_string(),
            text: "Last result".to_string(),
            success: true,
            tokens: 20,
            cost_usd: 0.02,
            duration_ms: 200,
            metadata: HashMap::new(),
        }).await;

        let first = manager.merge_results(MergeStrategy::FirstOnly).await;
        assert_eq!(first.text, "First result");

        let last = manager.merge_results(MergeStrategy::LastOnly).await;
        assert_eq!(last.text, "Last result");
    }

    #[tokio::test]
    async fn test_merge_success_only() {
        let manager = SharedStateManager::new();

        manager.store_result(AgentResultForMerge {
            agent_id: "success".to_string(),
            text: "Success".to_string(),
            success: true,
            tokens: 10,
            cost_usd: 0.01,
            duration_ms: 100,
            metadata: HashMap::new(),
        }).await;

        manager.store_result(AgentResultForMerge {
            agent_id: "failure".to_string(),
            text: "Failure".to_string(),
            success: false,
            tokens: 20,
            cost_usd: 0.02,
            duration_ms: 200,
            metadata: HashMap::new(),
        }).await;

        let merged = manager.merge_results(MergeStrategy::SuccessOnly).await;
        assert!(merged.text.contains("Success"));
        assert!(!merged.text.contains("Failure"));
        assert_eq!(merged.success_count, 1);
        assert_eq!(merged.failure_count, 1);
    }

    #[tokio::test]
    async fn test_metrics() {
        let manager = SharedStateManager::new();

        manager.set("key", serde_json::json!("value")).await;
        manager.broadcast_progress("agent", "progress", 0.5).await;
        manager.store_result(AgentResultForMerge {
            agent_id: "a".to_string(),
            text: "result".to_string(),
            success: true,
            tokens: 10,
            cost_usd: 0.01,
            duration_ms: 100,
            metadata: HashMap::new(),
        }).await;

        let metrics = manager.get_metrics().await;
        assert!(metrics.kv_operations >= 1);
        assert!(metrics.progress_broadcasts >= 1);
        assert!(metrics.results_stored >= 1);
    }

    #[tokio::test]
    async fn test_export_import() {
        let manager1 = SharedStateManager::new();

        manager1.set("key1", serde_json::json!("value1")).await;
        manager1.store_result(AgentResultForMerge {
            agent_id: "a".to_string(),
            text: "result".to_string(),
            success: true,
            tokens: 10,
            cost_usd: 0.01,
            duration_ms: 100,
            metadata: HashMap::new(),
        }).await;

        let exported = manager1.export().await;

        let manager2 = SharedStateManager::new();
        manager2.import(exported).await.expect("Import should succeed");

        assert_eq!(manager2.get("key1").await, Some(serde_json::json!("value1")));
        let results = manager2.get_results().await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_clone() {
        let manager1 = SharedStateManager::new();
        manager1.set("shared", serde_json::json!("data")).await;

        let manager2 = manager1.clone();
        
        // Both managers share the same underlying data
        assert_eq!(manager2.get("shared").await, Some(serde_json::json!("data")));
        
        manager2.set("shared", serde_json::json!("modified")).await;
        assert_eq!(manager1.get("shared").await, Some(serde_json::json!("modified")));
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let manager = Arc::new(SharedStateManager::new());
        let mut handles = vec![];

        // Spawn multiple concurrent writers
        for i in 0..10 {
            let m = Arc::clone(&manager);
            handles.push(tokio::spawn(async move {
                m.set(format!("key-{}", i), serde_json::json!(i)).await;
                m.increment("total", 1.0).await;
            }));
        }

        // Wait for all to complete
        for handle in handles {
            handle.await.expect("Task should complete");
        }

        // Verify all writes succeeded
        let total = manager.get("total").await;
        assert_eq!(total, Some(serde_json::json!(10.0)));

        for i in 0..10 {
            let value = manager.get(&format!("key-{}", i)).await;
            assert_eq!(value, Some(serde_json::json!(i)));
        }
    }
}
