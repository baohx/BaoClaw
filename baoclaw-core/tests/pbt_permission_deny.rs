//! PBT: Property 1 — Permission Deny prevents execution
//!
//! **Validates: Requirements 1.3, 1.5**
//!
//! For any tool call where PermissionManager returns Deny (either from rules
//! or from user decision), the tool's `call()` method shall never be invoked,
//! and the result shall be an error with is_error=true.

use proptest::prelude::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// We need to reference the crate items directly
use baoclaw_core::engine::query_engine::EngineEvent;
use baoclaw_core::permissions::gate::PermissionGate;
use baoclaw_core::permissions::manager::{
    PermissionManager, PermissionMode, PermissionRule, ToolPermissionContext,
};
use baoclaw_core::tools::executor::{execute_tool_with_permission, ToolUseRequest};
use baoclaw_core::tools::trait_def::*;

/// A mock tool that tracks whether call() was invoked.
struct TrackingTool {
    tool_name: String,
    call_count: Arc<AtomicUsize>,
}

impl TrackingTool {
    fn new(name: &str) -> Self {
        Self {
            tool_name: name.to_string(),
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl Tool for TrackingTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
            description: None,
        }
    }

    async fn call(
        &self,
        _input: Value,
        _context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult {
            data: json!({"result": "ok"}),
            is_error: false,
        })
    }

    fn prompt(&self) -> String {
        String::new()
    }
}

struct MockProgress;

#[async_trait::async_trait]
impl ProgressSender for MockProgress {
    async fn send_progress(&self, _tool_use_id: &str, _data: Value) {}
}

fn make_context() -> ToolContext {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolContext {
        cwd: PathBuf::from("/tmp"),
        model: "test".to_string(),
        abort_signal: Arc::new(rx),
    }
}

/// Strategy for generating tool names
fn tool_name_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[A-Za-z][A-Za-z0-9_]{0,20}")
        .unwrap()
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Strategy for generating tool_use_ids
fn tool_use_id_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("tu_[a-z0-9]{4,12}").unwrap()
}

/// Strategy for generating JSON input values
fn input_strategy() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(json!({})),
        Just(json!({"command": "ls"})),
        Just(json!({"path": "/tmp/test.txt"})),
        Just(json!({"pattern": "*.rs"})),
        any::<i32>().prop_map(|n| json!({"value": n})),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property 1: When PermissionManager returns Deny via deny rules,
    /// the tool's call() is never invoked and result is is_error=true.
    #[test]
    fn deny_rule_prevents_execution(
        tool_name in tool_name_strategy(),
        tool_use_id in tool_use_id_strategy(),
        input in input_strategy(),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (call_count_val, is_error, output_str) = rt.block_on(async {
            // Set up PermissionManager with a deny rule for this tool
            let mut deny_rules = HashMap::new();
            deny_rules.insert(
                "system".to_string(),
                vec![PermissionRule {
                    tool_name: tool_name.clone(),
                    rule_content: None,
                }],
            );

            let ctx = ToolPermissionContext {
                mode: PermissionMode::Default,
                additional_working_directories: HashMap::new(),
                always_allow_rules: HashMap::new(),
                always_deny_rules: deny_rules,
                always_ask_rules: HashMap::new(),
                is_bypass_permissions_mode_available: false,
            };

            let permission_manager = PermissionManager::new(ctx);
            let permission_gate = PermissionGate::new();
            let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<EngineEvent>(16);
            let progress = MockProgress;
            let tool_context = make_context();

            let tool = TrackingTool::new(&tool_name);
            let call_count = Arc::clone(&tool.call_count);

            let request = ToolUseRequest {
                id: tool_use_id,
                name: tool_name,
                input,
            };

            let result = execute_tool_with_permission(
                &tool,
                &request,
                &tool_context,
                &permission_manager,
                &permission_gate,
                &event_tx,
                &progress,
            )
            .await;

            (
                call_count.load(Ordering::SeqCst),
                result.is_error,
                result.output.as_str().unwrap_or("").to_string(),
            )
        });

        // Property: call() was never invoked
        prop_assert_eq!(call_count_val, 0,
            "Tool call() should not be invoked when permission is Deny");

        // Property: result is an error
        prop_assert!(is_error,
            "Result should be is_error=true when permission is Deny");

        // Property: output contains "Permission denied"
        prop_assert!(output_str.contains("Permission denied") || output_str.contains("denied"),
            "Output should contain denial message, got: {}", output_str);
    }
}
