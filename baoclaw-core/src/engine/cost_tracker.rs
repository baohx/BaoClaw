use std::collections::HashMap;

use crate::engine::query_engine::EMPTY_USAGE;
use crate::models::message::Usage;

/// Model pricing table (USD per million tokens).
#[derive(Clone, Debug)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_write_per_mtok: f64,
    pub cache_read_per_mtok: f64,
}

/// Default pricing for unknown models.
const DEFAULT_PRICING: ModelPricing = ModelPricing {
    input_per_mtok: 3.0,
    output_per_mtok: 15.0,
    cache_write_per_mtok: 3.75,
    cache_read_per_mtok: 0.30,
};

/// Cost tracker that calculates and accumulates API call costs.
pub struct CostTracker {
    pricing: HashMap<String, ModelPricing>,
    current_query_cost: f64,
    total_cost: f64,
    total_usage: Usage,
}

impl CostTracker {
    /// Create a new CostTracker with built-in pricing for known models.
    pub fn new() -> Self {
        let mut pricing = HashMap::new();

        // Claude Sonnet 4
        pricing.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input_per_mtok: 3.0,
                output_per_mtok: 15.0,
                cache_write_per_mtok: 3.75,
                cache_read_per_mtok: 0.30,
            },
        );

        // Claude Opus 4
        pricing.insert(
            "claude-opus-4-20250514".to_string(),
            ModelPricing {
                input_per_mtok: 15.0,
                output_per_mtok: 75.0,
                cache_write_per_mtok: 18.75,
                cache_read_per_mtok: 1.50,
            },
        );

        // Claude Haiku 3.5
        pricing.insert(
            "claude-3-5-haiku-20241022".to_string(),
            ModelPricing {
                input_per_mtok: 0.80,
                output_per_mtok: 4.0,
                cache_write_per_mtok: 1.0,
                cache_read_per_mtok: 0.08,
            },
        );

        Self {
            pricing,
            current_query_cost: 0.0,
            total_cost: 0.0,
            total_usage: EMPTY_USAGE,
        }
    }

    /// Calculate cost for a single API call based on usage and model.
    pub fn calculate_cost(&self, usage: &Usage, model: &str) -> f64 {
        let pricing = self.pricing.get(model).unwrap_or(&DEFAULT_PRICING);

        let input_cost = (usage.input_tokens as f64 / 1_000_000.0) * pricing.input_per_mtok;
        let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * pricing.output_per_mtok;
        let cache_write_cost = (usage.cache_creation_input_tokens.unwrap_or(0) as f64
            / 1_000_000.0)
            * pricing.cache_write_per_mtok;
        let cache_read_cost =
            (usage.cache_read_input_tokens.unwrap_or(0) as f64 / 1_000_000.0)
                * pricing.cache_read_per_mtok;

        input_cost + output_cost + cache_write_cost + cache_read_cost
    }

    /// Accumulate a single API call's usage into current query and total costs.
    pub fn accumulate(&mut self, usage: &Usage, model: &str) {
        let cost = self.calculate_cost(usage, model);
        self.current_query_cost += cost;
        self.total_cost += cost;

        // Accumulate token counts
        self.total_usage.input_tokens += usage.input_tokens;
        self.total_usage.output_tokens += usage.output_tokens;
        if let Some(cache_create) = usage.cache_creation_input_tokens {
            *self.total_usage.cache_creation_input_tokens.get_or_insert(0) += cache_create;
        }
        if let Some(cache_read) = usage.cache_read_input_tokens {
            *self.total_usage.cache_read_input_tokens.get_or_insert(0) += cache_read;
        }
    }

    /// Reset current query cost (called at the start of a new query).
    /// Preserves total_cost and total_usage.
    pub fn reset_query(&mut self) {
        self.current_query_cost = 0.0;
    }

    /// Get the current query cost.
    pub fn current_query_cost(&self) -> f64 {
        self.current_query_cost
    }

    /// Get the total accumulated cost across all queries.
    pub fn total_cost(&self) -> f64 {
        self.total_cost
    }

    /// Get the total accumulated usage across all queries.
    pub fn total_usage(&self) -> &Usage {
        &self.total_usage
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn make_usage(input: u64, output: u64, cache_create: Option<u64>, cache_read: Option<u64>) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            cache_creation_input_tokens: cache_create,
            cache_read_input_tokens: cache_read,
        }
    }

    // --- Construction ---

    #[test]
    fn test_new_tracker_has_zero_costs() {
        let tracker = CostTracker::new();
        assert_eq!(tracker.current_query_cost(), 0.0);
        assert_eq!(tracker.total_cost(), 0.0);
    }

    #[test]
    fn test_new_tracker_has_zero_usage() {
        let tracker = CostTracker::new();
        let usage = tracker.total_usage();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
    }

    #[test]
    fn test_new_tracker_has_builtin_pricing() {
        let tracker = CostTracker::new();
        // Sonnet 4 should be known
        let usage = make_usage(1_000_000, 0, None, None);
        let cost = tracker.calculate_cost(&usage, "claude-sonnet-4-20250514");
        assert!((cost - 3.0).abs() < 1e-10, "Sonnet 4 input cost for 1M tokens should be $3.0, got {}", cost);
    }

    // --- calculate_cost for each model ---

    #[test]
    fn test_calculate_cost_sonnet4_input_only() {
        let tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 0, None, None);
        let cost = tracker.calculate_cost(&usage, "claude-sonnet-4-20250514");
        assert!((cost - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_cost_sonnet4_output_only() {
        let tracker = CostTracker::new();
        let usage = make_usage(0, 1_000_000, None, None);
        let cost = tracker.calculate_cost(&usage, "claude-sonnet-4-20250514");
        assert!((cost - 15.0).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_cost_sonnet4_all_token_types() {
        let tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 1_000_000, Some(1_000_000), Some(1_000_000));
        let cost = tracker.calculate_cost(&usage, "claude-sonnet-4-20250514");
        // 3.0 + 15.0 + 3.75 + 0.30 = 22.05
        assert!((cost - 22.05).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_cost_opus4() {
        let tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 1_000_000, Some(1_000_000), Some(1_000_000));
        let cost = tracker.calculate_cost(&usage, "claude-opus-4-20250514");
        // 15.0 + 75.0 + 18.75 + 1.50 = 110.25
        assert!((cost - 110.25).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_cost_haiku35() {
        let tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 1_000_000, Some(1_000_000), Some(1_000_000));
        let cost = tracker.calculate_cost(&usage, "claude-3-5-haiku-20241022");
        // 0.80 + 4.0 + 1.0 + 0.08 = 5.88
        assert!((cost - 5.88).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_cost_unknown_model_uses_default() {
        let tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 1_000_000, None, None);
        let cost = tracker.calculate_cost(&usage, "unknown-model-v1");
        // Default: 3.0 + 15.0 = 18.0
        assert!((cost - 18.0).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_cost_zero_usage() {
        let tracker = CostTracker::new();
        let usage = make_usage(0, 0, None, None);
        let cost = tracker.calculate_cost(&usage, "claude-sonnet-4-20250514");
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_calculate_cost_small_usage() {
        let tracker = CostTracker::new();
        // 1000 input tokens with Sonnet 4: 1000/1M * 3.0 = 0.003
        let usage = make_usage(1000, 0, None, None);
        let cost = tracker.calculate_cost(&usage, "claude-sonnet-4-20250514");
        assert!((cost - 0.003).abs() < 1e-10);
    }

    // --- accumulate ---

    #[test]
    fn test_accumulate_adds_to_both_costs() {
        let mut tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 0, None, None);
        tracker.accumulate(&usage, "claude-sonnet-4-20250514");
        assert!((tracker.current_query_cost() - 3.0).abs() < 1e-10);
        assert!((tracker.total_cost() - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_accumulate_multiple_calls() {
        let mut tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 0, None, None);
        tracker.accumulate(&usage, "claude-sonnet-4-20250514");
        tracker.accumulate(&usage, "claude-sonnet-4-20250514");
        assert!((tracker.current_query_cost() - 6.0).abs() < 1e-10);
        assert!((tracker.total_cost() - 6.0).abs() < 1e-10);
    }

    #[test]
    fn test_accumulate_updates_total_usage() {
        let mut tracker = CostTracker::new();
        let usage = make_usage(100, 50, Some(20), Some(10));
        tracker.accumulate(&usage, "claude-sonnet-4-20250514");
        assert_eq!(tracker.total_usage().input_tokens, 100);
        assert_eq!(tracker.total_usage().output_tokens, 50);
        assert_eq!(tracker.total_usage().cache_creation_input_tokens, Some(20));
        assert_eq!(tracker.total_usage().cache_read_input_tokens, Some(10));
    }

    #[test]
    fn test_accumulate_usage_accumulates() {
        let mut tracker = CostTracker::new();
        let usage1 = make_usage(100, 50, None, None);
        let usage2 = make_usage(200, 30, Some(10), None);
        tracker.accumulate(&usage1, "claude-sonnet-4-20250514");
        tracker.accumulate(&usage2, "claude-sonnet-4-20250514");
        assert_eq!(tracker.total_usage().input_tokens, 300);
        assert_eq!(tracker.total_usage().output_tokens, 80);
        assert_eq!(tracker.total_usage().cache_creation_input_tokens, Some(10));
    }

    // --- reset_query ---

    #[test]
    fn test_reset_query_clears_current_cost() {
        let mut tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 0, None, None);
        tracker.accumulate(&usage, "claude-sonnet-4-20250514");
        assert!(tracker.current_query_cost() > 0.0);
        tracker.reset_query();
        assert_eq!(tracker.current_query_cost(), 0.0);
    }

    #[test]
    fn test_reset_query_preserves_total_cost() {
        let mut tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 0, None, None);
        tracker.accumulate(&usage, "claude-sonnet-4-20250514");
        let total_before = tracker.total_cost();
        tracker.reset_query();
        assert_eq!(tracker.total_cost(), total_before);
    }

    #[test]
    fn test_reset_then_accumulate() {
        let mut tracker = CostTracker::new();
        let usage = make_usage(1_000_000, 0, None, None);
        tracker.accumulate(&usage, "claude-sonnet-4-20250514");
        tracker.reset_query();
        tracker.accumulate(&usage, "claude-sonnet-4-20250514");
        // current_query_cost should be just the second call
        assert!((tracker.current_query_cost() - 3.0).abs() < 1e-10);
        // total_cost should be both calls
        assert!((tracker.total_cost() - 6.0).abs() < 1e-10);
    }
}
