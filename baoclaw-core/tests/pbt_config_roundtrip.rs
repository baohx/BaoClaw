//! PBT: Property P3 — Config round-trip
//! For any valid BaoclawConfig, save → load produces an equivalent config.

use proptest::prelude::*;
use std::collections::HashMap;

use baoclaw_core::config::{BaoclawConfig, load_config_from, save_config_to};

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
            primary_profile: None,
            model_profiles: Default::default(),
            fallback_profiles: Vec::new(),
            model,
            fallback_models,
            max_retries_per_model: max_retries,
            api_type: "anthropic".to_string(),
            openai_base_url: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
            tool_output_threshold_chars: 200_000,
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
