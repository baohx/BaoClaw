use std::time::Duration;

use serde_json::Value;
use tokio::time::timeout;

use crate::tools::trait_def::{ProgressSender, Tool, ToolContext, ToolError, ToolResult};

/// Execute a tool with a timeout. Returns ToolError::Timeout if exceeded.
pub async fn execute_tool_with_timeout(
    tool: &dyn Tool,
    input: Value,
    context: &ToolContext,
    progress: &dyn ProgressSender,
    timeout_ms: u64,
) -> Result<ToolResult, ToolError> {
    let duration = Duration::from_millis(timeout_ms);
    match timeout(duration, tool.call(input, context, progress)).await {
        Ok(result) => result,
        Err(_) => Err(ToolError::Timeout(timeout_ms)),
    }
}

/// Permission decision for non-interactive mode.
/// In non-interactive mode, automatically deny permission requests.
#[derive(Clone, Debug, PartialEq)]
pub enum AutoPermissionMode {
    Interactive,
    NonInteractive,
}

/// Auto-deny permission in non-interactive mode.
pub fn auto_permission_decision(mode: &AutoPermissionMode) -> Option<bool> {
    match mode {
        AutoPermissionMode::Interactive => None,        // Wait for user
        AutoPermissionMode::NonInteractive => Some(false), // Auto-deny
    }
}

/// Error recovery strategy for different error types.
#[derive(Clone, Debug)]
pub enum RecoveryStrategy {
    /// Retry with exponential backoff
    Retry {
        max_attempts: u32,
        initial_delay_ms: u64,
    },
    /// Request full state sync from Rust core
    FullStateSync,
    /// Restart the Rust core process
    RestartProcess,
    /// No recovery possible, report to user
    Fatal(String),
}

/// Determine the recovery strategy for a given error.
pub fn determine_recovery_strategy(error_type: &str, error_message: &str) -> RecoveryStrategy {
    match error_type {
        "ipc_disconnect" | "connection_closed" => RecoveryStrategy::RestartProcess,
        "state_sync_failed" | "patch_apply_failed" => RecoveryStrategy::FullStateSync,
        "api_rate_limited" | "api_server_error" => RecoveryStrategy::Retry {
            max_attempts: 3,
            initial_delay_ms: 1000,
        },
        "api_auth_error" | "api_bad_request" => {
            RecoveryStrategy::Fatal(error_message.to_string())
        }
        "mcp_disconnect" => RecoveryStrategy::Retry {
            max_attempts: 5,
            initial_delay_ms: 2000,
        },
        "tool_timeout" => {
            RecoveryStrategy::Fatal(format!("Tool timed out: {}", error_message))
        }
        _ => RecoveryStrategy::Fatal(error_message.to_string()),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::tools::trait_def::JsonSchema;

    /// A mock tool that completes instantly with a fixed result.
    struct InstantTool;

    #[async_trait]
    impl Tool for InstantTool {
        fn name(&self) -> &str {
            "instant"
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
            Ok(ToolResult {
                data: json!({"result": "ok"}),
                is_error: false,
            })
        }

        fn prompt(&self) -> String {
            "instant tool".to_string()
        }
    }

    /// A mock tool that sleeps for a configurable duration before returning.
    struct SlowTool {
        delay_ms: u64,
    }

    #[async_trait]
    impl Tool for SlowTool {
        fn name(&self) -> &str {
            "slow"
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
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            Ok(ToolResult {
                data: json!({"result": "slow_done"}),
                is_error: false,
            })
        }

        fn prompt(&self) -> String {
            "slow tool".to_string()
        }
    }

    /// A no-op progress sender for tests.
    struct NoopProgress;

    #[async_trait]
    impl ProgressSender for NoopProgress {
        async fn send_progress(&self, _tool_use_id: &str, _data: Value) {}
    }

    fn test_context() -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: PathBuf::from("/tmp"),
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
        }
    }

    // --- execute_tool_with_timeout tests ---

    #[tokio::test]
    async fn test_execute_tool_with_timeout_success() {
        let tool = InstantTool;
        let ctx = test_context();
        let progress = NoopProgress;

        let result =
            execute_tool_with_timeout(&tool, json!({}), &ctx, &progress, 5000).await;

        assert!(result.is_ok());
        let tool_result = result.unwrap();
        assert!(!tool_result.is_error);
        assert_eq!(tool_result.data["result"], "ok");
    }

    #[tokio::test]
    async fn test_execute_tool_with_timeout_times_out() {
        let tool = SlowTool { delay_ms: 500 };
        let ctx = test_context();
        let progress = NoopProgress;

        let result =
            execute_tool_with_timeout(&tool, json!({}), &ctx, &progress, 50).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::Timeout(ms) => assert_eq!(ms, 50),
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_execute_tool_with_timeout_completes_within_limit() {
        let tool = SlowTool { delay_ms: 10 };
        let ctx = test_context();
        let progress = NoopProgress;

        let result =
            execute_tool_with_timeout(&tool, json!({}), &ctx, &progress, 5000).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().data["result"], "slow_done");
    }

    // --- auto_permission_decision tests ---

    #[test]
    fn test_auto_permission_interactive_returns_none() {
        let result = auto_permission_decision(&AutoPermissionMode::Interactive);
        assert_eq!(result, None);
    }

    #[test]
    fn test_auto_permission_non_interactive_returns_false() {
        let result = auto_permission_decision(&AutoPermissionMode::NonInteractive);
        assert_eq!(result, Some(false));
    }

    // --- determine_recovery_strategy tests ---

    #[test]
    fn test_recovery_ipc_disconnect() {
        let strategy = determine_recovery_strategy("ipc_disconnect", "connection lost");
        assert!(matches!(strategy, RecoveryStrategy::RestartProcess));
    }

    #[test]
    fn test_recovery_connection_closed() {
        let strategy = determine_recovery_strategy("connection_closed", "peer closed");
        assert!(matches!(strategy, RecoveryStrategy::RestartProcess));
    }

    #[test]
    fn test_recovery_state_sync_failed() {
        let strategy = determine_recovery_strategy("state_sync_failed", "patch error");
        assert!(matches!(strategy, RecoveryStrategy::FullStateSync));
    }

    #[test]
    fn test_recovery_patch_apply_failed() {
        let strategy = determine_recovery_strategy("patch_apply_failed", "invalid path");
        assert!(matches!(strategy, RecoveryStrategy::FullStateSync));
    }

    #[test]
    fn test_recovery_api_rate_limited() {
        let strategy = determine_recovery_strategy("api_rate_limited", "429");
        match strategy {
            RecoveryStrategy::Retry {
                max_attempts,
                initial_delay_ms,
            } => {
                assert_eq!(max_attempts, 3);
                assert_eq!(initial_delay_ms, 1000);
            }
            other => panic!("Expected Retry, got {:?}", other),
        }
    }

    #[test]
    fn test_recovery_api_server_error() {
        let strategy = determine_recovery_strategy("api_server_error", "500");
        match strategy {
            RecoveryStrategy::Retry {
                max_attempts,
                initial_delay_ms,
            } => {
                assert_eq!(max_attempts, 3);
                assert_eq!(initial_delay_ms, 1000);
            }
            other => panic!("Expected Retry, got {:?}", other),
        }
    }

    #[test]
    fn test_recovery_api_auth_error() {
        let strategy = determine_recovery_strategy("api_auth_error", "invalid key");
        match strategy {
            RecoveryStrategy::Fatal(msg) => assert_eq!(msg, "invalid key"),
            other => panic!("Expected Fatal, got {:?}", other),
        }
    }

    #[test]
    fn test_recovery_api_bad_request() {
        let strategy = determine_recovery_strategy("api_bad_request", "malformed");
        match strategy {
            RecoveryStrategy::Fatal(msg) => assert_eq!(msg, "malformed"),
            other => panic!("Expected Fatal, got {:?}", other),
        }
    }

    #[test]
    fn test_recovery_mcp_disconnect() {
        let strategy = determine_recovery_strategy("mcp_disconnect", "server gone");
        match strategy {
            RecoveryStrategy::Retry {
                max_attempts,
                initial_delay_ms,
            } => {
                assert_eq!(max_attempts, 5);
                assert_eq!(initial_delay_ms, 2000);
            }
            other => panic!("Expected Retry, got {:?}", other),
        }
    }

    #[test]
    fn test_recovery_tool_timeout() {
        let strategy = determine_recovery_strategy("tool_timeout", "BashTool");
        match strategy {
            RecoveryStrategy::Fatal(msg) => assert!(msg.contains("Tool timed out")),
            other => panic!("Expected Fatal, got {:?}", other),
        }
    }

    #[test]
    fn test_recovery_unknown_error_type() {
        let strategy = determine_recovery_strategy("unknown_error", "something broke");
        match strategy {
            RecoveryStrategy::Fatal(msg) => assert_eq!(msg, "something broke"),
            other => panic!("Expected Fatal, got {:?}", other),
        }
    }
}
