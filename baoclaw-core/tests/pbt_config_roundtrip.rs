//! PBT: Property P3 — Config round-trip
//! For any valid BaoclawConfig, save → load produces an equivalent config.

use proptest::prelude::*;
use std::collections::HashMap;

use claude_core::config::{BaoclawConfig, load_config_from, save_config_to};

fn model_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("claude-sonnet-4-20250514".to_string()),
        Just("claude-opus-4-20250514".to_string()),
        Just("claude-3-5-haiku-20241022".to_string()),
        "[a-z\\-]{5,20}".prop_map(|s| s),
    ]
}

fn fallback_strategy() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(model_strategy(), 0..5)
}

fn config_strategy() -> impl Strategy<Value = BaoclawConfig> {
    (model_strategy(), fallback_strategy(), 1u32..10)
        .prop_map(|(model, fallback_models, max_retries)| BaoclawConfig {
            model,
            fallback_models,
            max_retries_per_model: max_retries,
            extra: HashMap::new(),
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn config_round_trip(config in config_strategy()) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        save_config_to(&config, &path).unwrap();
        let loaded = load_config_from(&path);

        prop_assert_eq!(&config, &loaded);
    }
}
