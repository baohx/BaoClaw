use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

use crate::tools::trait_def::*;

/// BashTool - executes shell commands via /bin/bash -c
pub struct BashTool;

impl BashTool {
    pub fn new() -> Self {
        Self
    }

    const DEFAULT_TIMEOUT_MS: u64 = 120_000;
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000)"
                }
            })),
            required: Some(vec!["command".to_string()]),
            description: Some("Execute a bash command".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn prompt(&self) -> String {
        "Execute bash commands. Use this to run shell commands on the system.".to_string()
    }

    async fn validate_input(
        &self,
        input: &Value,
        _context: &ToolContext,
    ) -> ValidationResult {
        match input.get("command").and_then(|v| v.as_str()) {
            Some(cmd) if !cmd.is_empty() => ValidationResult::Ok,
            _ => ValidationResult::Invalid {
                message: "Missing or empty 'command' field".to_string(),
                code: None,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'command' field".to_string()))?;

        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(Self::DEFAULT_TIMEOUT_MS);

        let timeout_duration = Duration::from_millis(timeout_ms);

        let mut child = tokio::process::Command::new("/bin/bash")
            .arg("-c")
            .arg(command)
            .current_dir(&context.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn bash: {}", e)))?;

        // Get child PID so we can kill it from the abort branch
        let child_id = child.id();

        // Race: wait for child vs timeout vs abort signal
        let abort_signal = context.abort_signal.clone();
        let result = tokio::select! {
            r = async {
                match tokio::time::timeout(timeout_duration, child.wait_with_output()).await {
                    Ok(Ok(output)) => Ok(output),
                    Ok(Err(e)) => Err(ToolError::ExecutionFailed(format!("Command execution failed: {}", e))),
                    Err(_) => Err(ToolError::Timeout(timeout_ms)),
                }
            } => r,
            _ = async {
                let mut rx = abort_signal.as_ref().clone();
                while !*rx.borrow() {
                    if rx.changed().await.is_err() { break; }
                }
            } => {
                // Kill the child process by PID
                if let Some(pid) = child_id {
                    unsafe { libc::kill(pid as i32, libc::SIGKILL); }
                }
                Err(ToolError::Aborted)
            }
        };

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = if stderr.is_empty() {
                    stdout.to_string()
                } else if stdout.is_empty() {
                    stderr.to_string()
                } else {
                    format!("{}\n{}", stdout, stderr)
                };

                let exit_code = output.status.code().unwrap_or(-1);
                let is_error = !output.status.success();

                Ok(ToolResult {
                    data: json!({
                        "stdout": stdout.as_ref(),
                        "stderr": stderr.as_ref(),
                        "exit_code": exit_code,
                        "output": combined,
                    }),
                    is_error,
                })
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct NoopProgress;
    #[async_trait]
    impl ProgressSender for NoopProgress {
        async fn send_progress(&self, _id: &str, _data: Value) {}
    }

    fn make_context() -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: PathBuf::from("/tmp"),
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
        }
    }

    #[tokio::test]
    async fn test_bash_echo() {
        let tool = BashTool::new();
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"command": "echo hello"}), &ctx, &progress)
            .await
            .unwrap();

        assert!(!result.is_error);
        let stdout = result.data.get("stdout").unwrap().as_str().unwrap();
        assert_eq!(stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_bash_failing_command() {
        let tool = BashTool::new();
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"command": "exit 1"}), &ctx, &progress)
            .await
            .unwrap();

        assert!(result.is_error);
        assert_eq!(result.data.get("exit_code").unwrap().as_i64().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_bash_timeout() {
        let tool = BashTool::new();
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({"command": "sleep 10", "timeout": 100}),
                &ctx,
                &progress,
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::Timeout(ms) => assert_eq!(ms, 100),
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_bash_validate_missing_command() {
        let tool = BashTool::new();
        let ctx = make_context();

        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[test]
    fn test_bash_properties() {
        let tool = BashTool::new();
        assert_eq!(tool.name(), "Bash");
        assert!(!tool.is_read_only(&json!({})));
        assert!(!tool.is_concurrency_safe(&json!({})));
    }
}
