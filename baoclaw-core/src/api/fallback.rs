// FallbackController — manages model fallback chain during rate-limit scenarios

use std::time::Duration;

use crate::api::retry::{calculate_backoff, RetryConfig};
use crate::config::BaoclawConfig;

/// Action to take after a rate-limit event.
#[derive(Clone, Debug)]
pub enum FallbackAction {
    /// Retry the same model with exponential backoff.
    Retry {
        model: String,
        attempt: u32,
        delay: Duration,
    },
    /// Fall back to the next model in the chain.
    Fallback {
        from: String,
        to: String,
    },
    /// All models in the chain have been exhausted.
    Exhausted {
        models_tried: Vec<String>,
        total_retries: u32,
    },
}

/// Manages the model fallback chain during a query.
pub struct FallbackController {
    chain: Vec<String>,
    current_index: usize,
    retry_count: u32,
    max_retries_per_model: u32,
    total_retries: u32,
}

impl FallbackController {
    /// Create a new FallbackController from config.
    /// The chain is [primary_model, fallback1, fallback2, ...].
    pub fn new(config: &BaoclawConfig) -> Self {
        let mut chain = vec![config.model.clone()];
        chain.extend(config.fallback_models.iter().cloned());
        Self {
            chain,
            current_index: 0,
            retry_count: 0,
            max_retries_per_model: config.max_retries_per_model,
            total_retries: 0,
        }
    }

    /// Called when a rate-limit (429) error is received.
    /// Returns the action to take: Retry, Fallback, or Exhausted.
    pub fn on_rate_limit(&mut self) -> FallbackAction {
        self.retry_count += 1;
        self.total_retries += 1;

        if self.retry_count < self.max_retries_per_model {
            // Still have retries left on current model
            let delay = calculate_backoff(
                self.retry_count - 1,
                &RetryConfig::default(),
            );
            FallbackAction::Retry {
                model: self.chain[self.current_index].clone(),
                attempt: self.retry_count,
                delay,
            }
        } else if self.current_index + 1 < self.chain.len() {
            // Move to next model in chain
            let from = self.chain[self.current_index].clone();
            self.current_index += 1;
            self.retry_count = 0;
            let to = self.chain[self.current_index].clone();
            FallbackAction::Fallback { from, to }
        } else {
            // All models exhausted
            FallbackAction::Exhausted {
                models_tried: self.chain.clone(),
                total_retries: self.total_retries,
            }
        }
    }

    /// Get the current model name.
    pub fn current_model(&self) -> &str {
        &self.chain[self.current_index]
    }

    /// Reset for a new query (back to primary model, zero retries).
    pub fn reset(&mut self) {
        self.current_index = 0;
        self.retry_count = 0;
        self.total_retries = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_config(model: &str, fallbacks: Vec<&str>, max_retries: u32) -> BaoclawConfig {
        BaoclawConfig {
            model: model.to_string(),
            fallback_models: fallbacks.into_iter().map(|s| s.to_string()).collect(),
            max_retries_per_model: max_retries,
            api_type: "anthropic".to_string(),
        openai_base_url: None,
            extra: HashMap::new(),
        }
    }

    #[test]
    fn test_single_model_retry() {
        let config = make_config("opus", vec![], 3);
        let mut fc = FallbackController::new(&config);

        assert_eq!(fc.current_model(), "opus");

        // First two rate limits → Retry
        match fc.on_rate_limit() {
            FallbackAction::Retry { model, attempt, .. } => {
                assert_eq!(model, "opus");
                assert_eq!(attempt, 1);
            }
            _ => panic!("Expected Retry"),
        }
        match fc.on_rate_limit() {
            FallbackAction::Retry { model, attempt, .. } => {
                assert_eq!(model, "opus");
                assert_eq!(attempt, 2);
            }
            _ => panic!("Expected Retry"),
        }

        // Third rate limit → Exhausted (no fallbacks)
        match fc.on_rate_limit() {
            FallbackAction::Exhausted { models_tried, total_retries } => {
                assert_eq!(models_tried, vec!["opus"]);
                assert_eq!(total_retries, 3);
            }
            _ => panic!("Expected Exhausted"),
        }
    }

    #[test]
    fn test_multi_model_fallback() {
        let config = make_config("opus", vec!["sonnet", "haiku"], 2);
        let mut fc = FallbackController::new(&config);

        assert_eq!(fc.current_model(), "opus");

        // First rate limit → Retry on opus
        match fc.on_rate_limit() {
            FallbackAction::Retry { model, .. } => assert_eq!(model, "opus"),
            _ => panic!("Expected Retry"),
        }

        // Second → Fallback to sonnet
        match fc.on_rate_limit() {
            FallbackAction::Fallback { from, to } => {
                assert_eq!(from, "opus");
                assert_eq!(to, "sonnet");
            }
            _ => panic!("Expected Fallback"),
        }
        assert_eq!(fc.current_model(), "sonnet");

        // Third → Retry on sonnet
        match fc.on_rate_limit() {
            FallbackAction::Retry { model, .. } => assert_eq!(model, "sonnet"),
            _ => panic!("Expected Retry"),
        }

        // Fourth → Fallback to haiku
        match fc.on_rate_limit() {
            FallbackAction::Fallback { from, to } => {
                assert_eq!(from, "sonnet");
                assert_eq!(to, "haiku");
            }
            _ => panic!("Expected Fallback"),
        }
        assert_eq!(fc.current_model(), "haiku");

        // Fifth → Retry on haiku
        match fc.on_rate_limit() {
            FallbackAction::Retry { model, .. } => assert_eq!(model, "haiku"),
            _ => panic!("Expected Retry"),
        }

        // Sixth → Exhausted
        match fc.on_rate_limit() {
            FallbackAction::Exhausted { models_tried, total_retries } => {
                assert_eq!(models_tried, vec!["opus", "sonnet", "haiku"]);
                assert_eq!(total_retries, 6);
            }
            _ => panic!("Expected Exhausted"),
        }
    }

    #[test]
    fn test_all_exhausted() {
        let config = make_config("opus", vec!["sonnet"], 1);
        let mut fc = FallbackController::new(&config);

        // max_retries=1, so first rate limit exhausts opus → fallback
        match fc.on_rate_limit() {
            FallbackAction::Fallback { from, to } => {
                assert_eq!(from, "opus");
                assert_eq!(to, "sonnet");
            }
            _ => panic!("Expected Fallback"),
        }

        // Second rate limit exhausts sonnet → exhausted
        match fc.on_rate_limit() {
            FallbackAction::Exhausted { models_tried, total_retries } => {
                assert_eq!(models_tried, vec!["opus", "sonnet"]);
                assert_eq!(total_retries, 2);
            }
            _ => panic!("Expected Exhausted"),
        }
    }

    #[test]
    fn test_reset() {
        let config = make_config("opus", vec!["sonnet"], 2);
        let mut fc = FallbackController::new(&config);

        // Advance to sonnet
        fc.on_rate_limit(); // retry opus
        fc.on_rate_limit(); // fallback to sonnet
        assert_eq!(fc.current_model(), "sonnet");

        // Reset
        fc.reset();
        assert_eq!(fc.current_model(), "opus");

        // Should be able to retry again from opus
        match fc.on_rate_limit() {
            FallbackAction::Retry { model, attempt, .. } => {
                assert_eq!(model, "opus");
                assert_eq!(attempt, 1);
            }
            _ => panic!("Expected Retry after reset"),
        }
    }
}
