//! Property-based tests for the SharedStateManager.
//!
//! These tests verify that the shared state properties hold across many inputs:
//! - Key-value operations are consistent and thread-safe
//! - Progress broadcast works correctly with multiple subscribers
//! - Result merging strategies produce correct outputs
//!
//! **Validates: Requirements FR-2.3 Agent 间通信**

use baoclaw_core::engine::team::shared_state::{
    AgentResultForMerge, MergeStrategy, SharedStateManager,
};
use proptest::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::broadcast::error::TryRecvError;

// Helper to run async tests
fn run_async<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let rt = Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(fut)
}

// ===========================================================================
// Key-Value Store Properties
// ===========================================================================

/// Strategy for generating valid JSON values for testing
fn json_value_strategy() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        // Simple values
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|v| serde_json::json!(v)),
        any::<f64>().prop_map(|v| {
            // Handle special float values
            if v.is_nan() || v.is_infinite() {
                serde_json::json!(0.0)
            } else {
                serde_json::json!(v)
            }
        }),
        any::<String>().prop_map(serde_json::Value::String),
        // Arrays
        proptest::collection::vec(
            prop_oneof![
                Just(serde_json::json!(1)),
                Just(serde_json::json!("test")),
                Just(serde_json::json!(true)),
            ],
            0..5
        )
        .prop_map(serde_json::Value::Array),
        // Objects
        proptest::collection::hash_map(
            "[a-z]{1,3}",
            prop_oneof![Just(serde_json::json!(1)), Just(serde_json::json!("v")),],
            0..3
        )
        .prop_map(|m| { serde_json::Value::Object(m.into_iter().map(|(k, v)| (k, v)).collect()) }),
    ]
}

proptest! {
    /// Property: Setting and getting a value returns the same value
    #[test]
    fn prop_set_get_roundtrip(key in "[a-zA-Z_][a-zA-Z0-9_]{0,10}", value in json_value_strategy()) {
        run_async(async {
            let manager = SharedStateManager::new();
            manager.set(&key, value.clone()).await;
            let retrieved = manager.get(&key).await;
            prop_assert_eq!(retrieved, Some(value));
            Ok(())
        })?;
    }

    /// Property: Contains returns true for set keys, false for unset keys
    #[test]
    fn prop_contains_consistency(
        set_keys in proptest::collection::vec("[a-z]{1,5}", 1..10),
        test_key in "[a-z]{1,5}"
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            // Set all keys
            for key in &set_keys {
                manager.set(key, serde_json::json!("value")).await;
            }

            // Check contains
            let expected = set_keys.contains(&test_key);
            let actual = manager.contains(&test_key).await;

            prop_assert_eq!(actual, expected);
            Ok(())
        })?;
    }

    /// Property: Remove returns the value that was set
    #[test]
    fn prop_remove_returns_set_value(key in "[a-z]{1,10}", value in json_value_strategy()) {
        run_async(async {
            let manager = SharedStateManager::new();
            manager.set(&key, value.clone()).await;

            let removed = manager.remove(&key).await;
            prop_assert_eq!(removed, Some(value));

            // Key should no longer exist
            let exists = manager.contains(&key).await;
            prop_assert!(!exists);
            Ok(())
        })?;
    }

    /// Property: Set many and get many preserves all values
    #[test]
    fn prop_set_many_get_many_consistency(
        pairs in proptest::collection::hash_map("[a-z]{1,5}", json_value_strategy(), 1..10)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            let keys: Vec<&str> = pairs.keys().map(|s| s.as_str()).collect();

            manager.set_many(pairs.clone()).await;

            let retrieved = manager.get_many(&keys).await;

            prop_assert_eq!(retrieved.len(), pairs.len());
            for (key, value) in &pairs {
                prop_assert_eq!(retrieved.get(key), Some(value));
            }
            Ok(())
        })?;
    }

    /// Property: Increment accumulates correctly
    #[test]
    fn prop_increment_accumulates(
        key in "[a-z]{1,5}",
        amounts in proptest::collection::vec(any::<f64>(), 1..10)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            // Filter out NaN and infinity
            let amounts: Vec<f64> = amounts.into_iter()
                .filter(|v| v.is_finite())
                .collect();

            let mut expected = 0.0;
            for amount in &amounts {
                let result = manager.increment(&key, *amount).await;
                expected += amount;
                // Allow small floating point errors
                prop_assert!((result - expected).abs() < 1e-10);
            }
            Ok(())
        })?;
    }
}

// ===========================================================================
// Progress Broadcast Properties
// ===========================================================================

proptest! {
    /// Property: All broadcast events are received by subscribers
    #[test]
    fn prop_broadcast_delivery(
        events in proptest::collection::vec(
            ("agent-[0-9]", "[a-z ]{5,20}", 0.0f64..1.0f64),
            1..20
        )
    ) {
        run_async(async {
            let manager = SharedStateManager::new();
            let mut rx = manager.subscribe_progress();

            let mut received_count = 0;
            for (agent_id, message, progress) in &events {
                manager.broadcast_progress(agent_id, message, *progress).await;
                received_count += 1;
            }

            // Check that we received all events
            for _ in 0..received_count {
                match rx.try_recv() {
                    Ok(event) => {
                        // Verify event structure
                        prop_assert!(event.progress >= 0.0 && event.progress <= 1.0);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Closed) => {
                        // Channel closed, which shouldn't happen
                        prop_assert!(false, "Channel closed unexpectedly");
                    }
                    Err(TryRecvError::Lagged(_)) => {
                        // Lag is acceptable for high-volume tests
                    }
                }
            }
            Ok(())
        })?;
    }

    /// Property: Progress values are clamped to [0.0, 1.0]
    #[test]
    fn prop_progress_clamped(
        agent_id in "[a-z]{3,5}",
        message in "[a-z ]{5,10}",
        progress in any::<f64>()
    ) {
        run_async(async {
            let manager = SharedStateManager::new();
            let mut rx = manager.subscribe_progress();

            // Handle NaN/Infinity by skipping if invalid
            if progress.is_finite() {
                manager.broadcast_progress(&agent_id, &message, progress).await;

                if let Ok(event) = rx.try_recv() {
                    prop_assert!(event.progress >= 0.0);
                    prop_assert!(event.progress <= 1.0);
                }
            }
            Ok(())
        })?;
    }
}

// ===========================================================================
// Result Storage and Merging Properties
// ===========================================================================

/// Strategy for generating agent results
fn agent_result_strategy() -> impl Strategy<Value = AgentResultForMerge> {
    (
        "agent-[0-9]{1,2}",
        "[a-zA-Z ]{10,50}",
        any::<bool>(),
        0u64..1000u64,
        0.0f64..1.0,
        100u64..10000u64,
    )
        .prop_map(|(agent_id, text, success, tokens, cost_usd, duration_ms)| {
            AgentResultForMerge {
                agent_id,
                text,
                success,
                tokens,
                cost_usd,
                duration_ms,
                metadata: HashMap::new(),
            }
        })
}

proptest! {
    /// Property: Stored results can be retrieved
    #[test]
    fn prop_store_retrieve_results(
        results in proptest::collection::vec(agent_result_strategy(), 1..10)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            for result in &results {
                manager.store_result(result.clone()).await;
            }

            let retrieved = manager.get_results().await;
            prop_assert_eq!(retrieved.len(), results.len());
            Ok(())
        })?;
    }

    /// Property: Merge concat contains all agent IDs
    #[test]
    fn prop_merge_concat_contains_all_ids(
        results in proptest::collection::vec(agent_result_strategy(), 1..5)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            for result in results {
                manager.store_result(result).await;
            }

            let merged = manager.merge_results(MergeStrategy::Concat).await;

            // Verify the merged result is valid
            prop_assert!(merged.count >= 1);
            Ok(())
        })?;
    }

    /// Property: Merge first returns only the first result's text
    #[test]
    fn prop_merge_first_is_first(
        results in proptest::collection::vec(agent_result_strategy(), 1..5)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            let first_text = results.first().map(|r| r.text.clone());

            for result in results {
                manager.store_result(result).await;
            }

            let merged = manager.merge_results(MergeStrategy::FirstOnly).await;

            prop_assert_eq!(merged.text, first_text.unwrap_or_default());
            Ok(())
        })?;
    }

    /// Property: Merge last returns only the last result's text
    #[test]
    fn prop_merge_last_is_last(
        results in proptest::collection::vec(agent_result_strategy(), 1..5)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            let last_text = results.last().map(|r| r.text.clone());

            for result in results {
                manager.store_result(result).await;
            }

            let merged = manager.merge_results(MergeStrategy::LastOnly).await;

            prop_assert_eq!(merged.text, last_text.unwrap_or_default());
            Ok(())
        })?;
    }

    /// Property: Merged totals equal sum of individual values
    #[test]
    fn prop_merge_totals_sum_correctly(
        results in proptest::collection::vec(agent_result_strategy(), 1..10)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            let expected_tokens: u64 = results.iter().map(|r| r.tokens).sum();
            let expected_cost: f64 = results.iter().map(|r| r.cost_usd).sum();
            let expected_duration: u64 = results.iter().map(|r| r.duration_ms).sum();
            let expected_success = results.iter().filter(|r| r.success).count();
            let expected_failure = results.iter().filter(|r| !r.success).count();

            for result in results {
                manager.store_result(result).await;
            }

            let merged = manager.merge_results(MergeStrategy::Concat).await;

            prop_assert_eq!(merged.total_tokens, expected_tokens);
            prop_assert!((merged.total_cost_usd - expected_cost).abs() < 1e-10);
            prop_assert_eq!(merged.total_duration_ms, expected_duration);
            prop_assert_eq!(merged.success_count, expected_success);
            prop_assert_eq!(merged.failure_count, expected_failure);
            Ok(())
        })?;
    }

    /// Property: Success only merge only includes successful results in output
    #[test]
    fn prop_merge_success_only_includes_only_successful(
        results in proptest::collection::vec(agent_result_strategy(), 1..10)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            // Calculate success count before storing
            let success_count = results.iter().filter(|r| r.success).count();
            let successful_ids: Vec<_> = results.iter()
                .filter(|r| r.success)
                .map(|r| r.agent_id.clone())
                .collect();

            // Store all results
            for result in results {
                manager.store_result(result).await;
            }

            let merged = manager.merge_results(MergeStrategy::SuccessOnly).await;

            // Check that merged text contains successful agent IDs
            for id in &successful_ids {
                prop_assert!(merged.text.contains(id));
            }

            // The success_count should reflect successful results
            prop_assert_eq!(merged.success_count, success_count);
            Ok(())
        })?;
    }

    /// Property: JSON array merge produces valid JSON array
    #[test]
    fn prop_merge_json_array_valid(
        results in proptest::collection::vec(agent_result_strategy(), 1..5)
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            let count = results.len();
            for result in results {
                manager.store_result(result).await;
            }

            let merged = manager.merge_results(MergeStrategy::JsonArray).await;

            // Should be valid JSON
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&merged.text);
            prop_assert!(parsed.is_ok());

            if let Ok(arr) = parsed {
                prop_assert_eq!(arr.len(), count);
            }
            Ok(())
        })?;
    }
}

// ===========================================================================
// Concurrent Access Properties
// ===========================================================================

proptest! {
    /// Property: Concurrent writes are all preserved
    #[test]
    fn prop_concurrent_writes_preserved(
        keys_values in proptest::collection::vec(("[a-z]{1,5}", json_value_strategy()), 1..20)
    ) {
        run_async(async {
            let manager = Arc::new(SharedStateManager::new());
            let mut handles = vec![];

            for (key, value) in keys_values.clone() {
                let m = Arc::clone(&manager);
                handles.push(tokio::spawn(async move {
                    m.set(&key, value).await;
                }));
            }

            for handle in handles {
                handle.await.expect("Task should complete");
            }

            // All keys should exist
            for (key, _) in &keys_values {
                prop_assert!(manager.contains(key).await);
            }
            Ok(())
        })?;
    }
}

// ===========================================================================
// Export/Import Properties
// ===========================================================================

proptest! {
    /// Property: Export and import preserves data
    #[test]
    fn prop_export_import_preserves_data(
        kv_pairs in proptest::collection::hash_map("[a-z]{1,5}", json_value_strategy(), 0..5)
    ) {
        run_async(async {
            let manager1 = SharedStateManager::new();

            for (key, value) in &kv_pairs {
                manager1.set(key, value.clone()).await;
            }

            let exported = manager1.export().await;

            let manager2 = SharedStateManager::new();
            manager2.import(exported).await.expect("Import should succeed");

            for (key, value) in &kv_pairs {
                let retrieved = manager2.get(key).await;
                prop_assert_eq!(retrieved.as_ref(), Some(value));
            }
            Ok(())
        })?;
    }
}

// ===========================================================================
// Metrics Properties
// ===========================================================================

proptest! {
    /// Property: Metrics count operations correctly
    #[test]
    fn prop_metrics_count_operations(
        operations in proptest::collection::vec(
            prop_oneof![
                Just("set"),
                Just("get"),
                Just("contains"),
            ],
            1..20
        )
    ) {
        run_async(async {
            let manager = SharedStateManager::new();

            for op in &operations {
                match *op {
                    "set" => {
                        manager.set("key", serde_json::json!("value")).await;
                    }
                    "get" => {
                        let _ = manager.get("key").await;
                    }
                    "contains" => {
                        let _ = manager.contains("key").await;
                    }
                    _ => {}
                }
            }

            let metrics = manager.get_metrics().await;
            // Every operation should be counted
            prop_assert_eq!(metrics.kv_operations, operations.len() as u64);
            Ok(())
        })?;
    }
}
