//! PBT: Property 11 — Cost calculation formula
//! PBT: Property 12 — Cost accumulation invariant
//!
//! **Validates: Requirements 5.1, 5.2, 5.3, 5.4, 5.5**

use proptest::prelude::*;

use baoclaw_core::engine::cost_tracker::CostTracker;
use baoclaw_core::models::message::Usage;

/// Strategy for generating arbitrary Usage values.
fn usage_strategy() -> impl Strategy<Value = Usage> {
    (
        0u64..10_000_000,
        0u64..10_000_000,
        prop::option::of(0u64..10_000_000),
        prop::option::of(0u64..10_000_000),
    )
        .prop_map(|(input, output, cache_create, cache_read)| Usage {
            input_tokens: input,
            output_tokens: output,
            cache_creation_input_tokens: cache_create,
            cache_read_input_tokens: cache_read,
        })
}

/// Strategy for generating model names (known + unknown).
fn model_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("claude-sonnet-4-20250514".to_string()),
        Just("claude-opus-4-20250514".to_string()),
        Just("claude-3-5-haiku-20241022".to_string()),
        Just("unknown-model".to_string()),
    ]
}

/// Get the expected pricing for a model.
fn expected_pricing(model: &str) -> (f64, f64, f64, f64) {
    match model {
        "claude-sonnet-4-20250514" => (3.0, 15.0, 3.75, 0.30),
        "claude-opus-4-20250514" => (15.0, 75.0, 18.75, 1.50),
        "claude-3-5-haiku-20241022" => (0.80, 4.0, 1.0, 0.08),
        _ => (3.0, 15.0, 3.75, 0.30), // default
    }
}

/// Compute expected cost using the formula directly.
fn expected_cost(usage: &Usage, model: &str) -> f64 {
    let (input_rate, output_rate, cache_write_rate, cache_read_rate) = expected_pricing(model);
    let input_cost = (usage.input_tokens as f64 / 1_000_000.0) * input_rate;
    let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * output_rate;
    let cache_write_cost =
        (usage.cache_creation_input_tokens.unwrap_or(0) as f64 / 1_000_000.0) * cache_write_rate;
    let cache_read_cost =
        (usage.cache_read_input_tokens.unwrap_or(0) as f64 / 1_000_000.0) * cache_read_rate;
    input_cost + output_cost + cache_write_cost + cache_read_cost
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property 11: Cost calculation formula
    ///
    /// **Validates: Requirements 5.1, 5.4, 5.5**
    ///
    /// For any Usage and model name, calculate_cost returns the exact
    /// formula result: (input/1M * input_rate) + (output/1M * output_rate)
    /// + (cache_create/1M * cache_write_rate) + (cache_read/1M * cache_read_rate)
    #[test]
    fn cost_calculation_matches_formula(
        usage in usage_strategy(),
        model in model_strategy(),
    ) {
        let tracker = CostTracker::new();
        let actual = tracker.calculate_cost(&usage, &model);
        let expected = expected_cost(&usage, &model);

        // Use relative tolerance for floating point comparison
        let diff = (actual - expected).abs();
        let tolerance = expected.abs() * 1e-10 + 1e-15;
        prop_assert!(
            diff <= tolerance,
            "Cost mismatch for model '{}': actual={}, expected={}, diff={}",
            model, actual, expected, diff
        );
    }

    /// Property 11b: Cost is always non-negative
    ///
    /// **Validates: Requirements 5.1**
    ///
    /// For any Usage and model, the calculated cost is >= 0.
    #[test]
    fn cost_is_always_non_negative(
        usage in usage_strategy(),
        model in model_strategy(),
    ) {
        let tracker = CostTracker::new();
        let cost = tracker.calculate_cost(&usage, &model);
        prop_assert!(cost >= 0.0, "Cost should be non-negative, got {}", cost);
    }

    /// Property 12: Cost accumulation invariant
    ///
    /// **Validates: Requirements 5.2, 5.3**
    ///
    /// For any N calls, total_cost equals the sum of individual calculate_cost values.
    /// After reset_query, current_query_cost=0 but total_cost is unchanged.
    #[test]
    fn cost_accumulation_invariant(
        usages in prop::collection::vec(usage_strategy(), 1..20),
        model in model_strategy(),
    ) {
        let mut tracker = CostTracker::new();

        let mut expected_total = 0.0;
        for usage in &usages {
            let individual_cost = tracker.calculate_cost(usage, &model);
            expected_total += individual_cost;
            tracker.accumulate(usage, &model);
        }

        // total_cost should equal sum of individual costs
        let diff = (tracker.total_cost() - expected_total).abs();
        let tolerance = expected_total.abs() * 1e-10 + 1e-15;
        prop_assert!(
            diff <= tolerance,
            "Total cost mismatch: actual={}, expected={}, diff={}",
            tracker.total_cost(), expected_total, diff
        );

        // current_query_cost should also equal the sum (no reset yet)
        let query_diff = (tracker.current_query_cost() - expected_total).abs();
        prop_assert!(
            query_diff <= tolerance,
            "Query cost mismatch: actual={}, expected={}",
            tracker.current_query_cost(), expected_total
        );

        // After reset, current_query_cost=0 but total_cost unchanged
        let total_before_reset = tracker.total_cost();
        tracker.reset_query();
        prop_assert_eq!(
            tracker.current_query_cost(),
            0.0,
            "current_query_cost should be 0 after reset"
        );
        let reset_diff = (tracker.total_cost() - total_before_reset).abs();
        prop_assert!(
            reset_diff < 1e-15,
            "total_cost should be unchanged after reset: before={}, after={}",
            total_before_reset, tracker.total_cost()
        );
    }

    /// Property 12b: Multi-query accumulation
    ///
    /// **Validates: Requirements 5.2, 5.3**
    ///
    /// Across multiple queries (with resets between them), total_cost
    /// equals the sum of all individual costs, and current_query_cost
    /// only reflects the current query.
    #[test]
    fn multi_query_accumulation(
        query1_usages in prop::collection::vec(usage_strategy(), 1..10),
        query2_usages in prop::collection::vec(usage_strategy(), 1..10),
        model in model_strategy(),
    ) {
        let mut tracker = CostTracker::new();

        // First query
        let mut total_expected = 0.0;
        for usage in &query1_usages {
            let cost = tracker.calculate_cost(usage, &model);
            total_expected += cost;
            tracker.accumulate(usage, &model);
        }

        // Reset for second query
        tracker.reset_query();
        prop_assert_eq!(tracker.current_query_cost(), 0.0);

        // Second query
        let mut query2_expected = 0.0;
        for usage in &query2_usages {
            let cost = tracker.calculate_cost(usage, &model);
            total_expected += cost;
            query2_expected += cost;
            tracker.accumulate(usage, &model);
        }

        // current_query_cost should reflect only query 2
        let query_diff = (tracker.current_query_cost() - query2_expected).abs();
        let query_tol = query2_expected.abs() * 1e-10 + 1e-15;
        prop_assert!(
            query_diff <= query_tol,
            "Query 2 cost mismatch: actual={}, expected={}",
            tracker.current_query_cost(), query2_expected
        );

        // total_cost should reflect both queries
        let total_diff = (tracker.total_cost() - total_expected).abs();
        let total_tol = total_expected.abs() * 1e-10 + 1e-15;
        prop_assert!(
            total_diff <= total_tol,
            "Total cost mismatch: actual={}, expected={}",
            tracker.total_cost(), total_expected
        );
    }
}
