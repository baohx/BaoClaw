//! PBT: Property P3 — Config round-trip
//!
//! `load_config_from` documents an auto-migration contract: legacy (profile-less)
//! configs are normalized into the profile format on load (`normalize_profiles`
//! + `sync_profiles_to_legacy`). Therefore the correct property is:
//!
//!   save(config) → load == migrate(config)
//!
//! where `migrate` applies the same documented pipeline. Additionally, loading
//! must be idempotent: once migrated, a second round-trip is a fixed point.

use proptest::prelude::*;
use std::collections::HashMap;

use baoclaw_core::config::{
    BaoclawConfig, load_config_from, normalize_profiles, save_config_to, sync_profiles_to_legacy,
};

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

/// The documented on-load migration pipeline, applied to an in-memory config.
fn migrate(config: &BaoclawConfig) -> BaoclawConfig {
    let mut migrated = config.clone();
    normalize_profiles(&mut migrated);
    sync_profiles_to_legacy(&mut migrated);
    migrated
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn config_round_trip(config in config_strategy()) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        save_config_to(&config, &path).unwrap();
        let loaded = load_config_from(&path);

        // Round-trip preserves the config modulo the documented migration.
        prop_assert_eq!(&migrate(&config), &loaded);

        // Idempotence: a second round-trip of the loaded config is a fixed point.
        let path2 = dir.path().join("config2.json");
        save_config_to(&loaded, &path2).unwrap();
        let reloaded = load_config_from(&path2);
        prop_assert_eq!(&loaded, &reloaded);
    }
}
