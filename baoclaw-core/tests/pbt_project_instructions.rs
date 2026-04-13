//! PBT: Property 5 — Project instructions injection
//!
//! **Validates: Requirements 2.2, 2.5**
//!
//! For any non-empty file content written to BAOCLAW.md, the system prompt
//! built by build_system_prompt should contain that content.

use proptest::prelude::*;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

use baoclaw_core::api::client::{AnthropicClient, ApiClientConfig};
use baoclaw_core::engine::query_engine::{
    build_system_prompt, load_project_instructions, QueryLoopConfig, ThinkingConfig,
};

/// Strategy for generating non-empty, non-whitespace-only instruction content.
fn instruction_content_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[A-Za-z0-9 _.,:;!?\\-]{1,200}")
        .unwrap()
        .prop_filter("non-empty after trim", |s| !s.trim().is_empty())
}

fn make_api_client() -> Arc<AnthropicClient> {
    Arc::new(AnthropicClient::new(ApiClientConfig {
        api_key: "test-key".to_string(),
        base_url: None,
        max_retries: None,
    }))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property 5: For any non-empty file content, the system prompt should
    /// contain that content under the project instructions header.
    #[test]
    fn project_instructions_injected_into_system_prompt(
        content in instruction_content_strategy(),
    ) {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);

        let config = QueryLoopConfig {
            api_client: make_api_client(),
            tools: vec![],
            model: "test-model".to_string(),
            max_turns: None,
            cwd: PathBuf::from("/tmp"),
            custom_system_prompt: None,
            append_system_prompt: None,
            project_instructions: Some(content.clone()),
            git_info: None,
            thinking_config: ThinkingConfig::Disabled,
            abort_rx,
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
        };

        let system = build_system_prompt(&config);
        prop_assert!(system.is_some(), "System prompt should not be None");

        let blocks = system.unwrap();
        prop_assert!(!blocks.is_empty(), "System prompt blocks should not be empty");

        let text = blocks[0]["text"].as_str().unwrap_or("");

        // The system prompt must contain the project instructions header
        prop_assert!(
            text.contains("# Project Instructions (from BAOCLAW.md)"),
            "System prompt should contain the project instructions header"
        );

        // The system prompt must contain the actual content
        prop_assert!(
            text.contains(&content),
            "System prompt should contain the instruction content: '{}'", content
        );
    }

    /// Property 5 (file round-trip): For any non-empty content written to
    /// BAOCLAW.md, load_project_instructions returns that content.
    #[test]
    fn project_instructions_loaded_from_file(
        content in instruction_content_strategy(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("BAOCLAW.md"), &content).unwrap();

        let loaded = load_project_instructions(dir.path());
        prop_assert!(loaded.is_some(), "Should load non-empty file");
        prop_assert_eq!(loaded.unwrap(), content);
    }

    /// Property 5 (priority): When both files exist with non-empty content,
    /// .baoclaw/BAOCLAW.md always takes priority.
    #[test]
    fn baoclaw_dir_takes_priority(
        priority_content in instruction_content_strategy(),
        fallback_content in instruction_content_strategy(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let baoclaw_dir = dir.path().join(".baoclaw");
        std::fs::create_dir_all(&baoclaw_dir).unwrap();
        std::fs::write(baoclaw_dir.join("BAOCLAW.md"), &priority_content).unwrap();
        std::fs::write(dir.path().join("BAOCLAW.md"), &fallback_content).unwrap();

        let loaded = load_project_instructions(dir.path());
        prop_assert!(loaded.is_some());
        prop_assert_eq!(loaded.unwrap(), priority_content);
    }
}
