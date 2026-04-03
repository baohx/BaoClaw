//! PBT: Property P1 — Total attempts before exhaustion = (N+1) * R
//! PBT: Property P2 — current_model() always returns a model from the chain

use proptest::prelude::*;
use std::collections::HashMap;

use claude_core::api::fallback::{FallbackAction, FallbackController};
use claude_core::config::BaoclawConfig;

fn make_config(n_fallbacks: usize, max_retries: u32) -> BaoclawConfig {
    let mut fallbacks = Vec::new();
    for i in 0..n_fallbacks {
        fallbacks.push(format!("fallback-model-{}", i));
    }
    BaoclawConfig {
        model: "primary-model".to_string(),
        fallback_models: fallbacks,
        max_retries_per_model: max_retries,
        extra: HashMap::new(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// P1: Total attempts before exhaustion = (N+1) * max_retries
    #[test]
    fn total_attempts_before_exhaustion(
        n_fallbacks in 0usize..8,
        max_retries in 1u32..6,
    ) {
        let config = make_config(n_fallbacks, max_retries);
        let mut fc = FallbackController::new(&config);
        let total_models = n_fallbacks + 1;
        let expected_total = (total_models as u32) * max_retries;

        let mut count = 0u32;
        loop {
            match fc.on_rate_limit() {
                FallbackAction::Retry { .. } => { count += 1; }
                FallbackAction::Fallback { .. } => { count += 1; }
                FallbackAction::Exhausted { total_retries, .. } => {
                    prop_assert_eq!(
                        total_retries, expected_total,
                        "Expected {} total retries for {} models * {} retries, got {}",
                        expected_total, total_models, max_retries, total_retries
                    );
                    break;
                }
            }
            // Safety: prevent infinite loop
            if count > 1000 { prop_assert!(false, "Too many iterations"); break; }
        }
    }

    /// P2: current_model() always returns a model from the chain
    #[test]
    fn current_model_always_in_chain(
        n_fallbacks in 0usize..8,
        max_retries in 1u32..6,
        num_calls in 1usize..30,
    ) {
        let config = make_config(n_fallbacks, max_retries);
        let mut fc = FallbackController::new(&config);

        let mut chain = vec!["primary-model".to_string()];
        for i in 0..n_fallbacks {
            chain.push(format!("fallback-model-{}", i));
        }

        // Before any calls
        prop_assert!(
            chain.contains(&fc.current_model().to_string()),
            "Initial model '{}' not in chain", fc.current_model()
        );

        for _ in 0..num_calls {
            match fc.on_rate_limit() {
                FallbackAction::Exhausted { .. } => break,
                _ => {}
            }
            prop_assert!(
                chain.contains(&fc.current_model().to_string()),
                "Model '{}' not in chain {:?}", fc.current_model(), chain
            );
        }
    }
}
