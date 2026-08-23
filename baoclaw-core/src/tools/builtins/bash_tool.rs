use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::engine::sandbox::SandboxConfig;
use crate::tools::trait_def::*;

/// BashTool - executes shell commands via /bin/bash -c
///
/// Optionally wraps commands through a sandbox (Bubblewrap / Docker) when
/// a `SandboxConfig` with a non-None backend is provided.
pub struct BashTool {
    sandbox: Option<Arc<SandboxConfig>>,
}

impl BashTool {
    /// Create a BashTool with no sandbox (direct execution).
    pub fn new() -> Self {
        Self { sandbox: None }
    }

    /// Create a BashTool with the given sandbox configuration.
    pub fn with_sandbox(sandbox: Arc<SandboxConfig>) -> Self {
        Self {
            sandbox: Some(sandbox),
        }
    }

    const DEFAULT_TIMEOUT_MS: u64 = 120_000;

    /// Build the actual command parts to execute.
    /// Returns (program, args) — either sandboxed or direct.
    fn build_command(&self, raw_command: &str, cwd: &Path) -> (String, Vec<String>) {
        if let Some(ref sandbox_cfg) = self.sandbox {
            match &sandbox_cfg.backend {
                crate::engine::sandbox::SandboxBackend::None => {
                    // SandboxConfig exists but backend is None → direct execution
                    ("/bin/bash".to_string(), vec!["-c".to_string(), raw_command.to_string()])
                }
                _ => {
                    // Build sandbox command as proper argument vector (no shell wrapping)
                    let args = sandbox_cfg.build_command_args(raw_command, cwd);
                    // First element is the program, rest are args
                    if args.is_empty() {
                        ("/bin/bash".to_string(), vec!["-c".to_string(), raw_command.to_string()])
                    } else {
                        let program = args[0].clone();
                        let cmd_args = args[1..].to_vec();
                        (program, cmd_args)
                    }
                }
            }
        } else {
            // No sandbox config at all → direct execution
            ("/bin/bash".to_string(), vec!["-c".to_string(), raw_command.to_string()])
        }
    }
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
        let sandbox_desc = match &self.sandbox {
            Some(cfg) => format!(" (sandbox: {})", cfg.description()),
            None => String::new(),
        };
        format!(
            "Execute bash commands. Use this to run shell commands on the system.{}",
            sandbox_desc
        )
    }

    async fn validate_input(
        &self,
        input: &Value,
        _context: &ToolContext,
    ) -> ValidationResult {
        match input.get("command").and_then(|v| v.as_str()) {
            Some(cmd) if !cmd.is_empty() => {
                // Security: check against dangerous command blocklist
                if let Err(reason) = crate::engine::security::check_dangerous_command(cmd) {
                    return ValidationResult::Invalid {
                        message: format!("Command blocked for safety: {}", reason),
                        code: Some("DANGEROUS_COMMAND".to_string()),
                    };
                }
                ValidationResult::Ok
            }
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

        // Build command — sandboxed or direct
        let (program, args) = self.build_command(command, &context.cwd);

        let mut cmd = tokio::process::Command::new(&program);
        for arg in &args {
            cmd.arg(arg);
        }
        let child = cmd
            .current_dir(&context.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "Failed to spawn '{}' (sandbox: {}): {}",
                    program,
                    self.sandbox
                        .as_ref()
                        .map(|s| s.description())
                        .unwrap_or("none"),
                    e
                ))
            })?;

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
                // Only abort if the value actually changed to true.
                // A dropped sender with value=false is not an abort.
                loop {
                    if *rx.borrow() {
                        break; // abort signal received
                    }
                    if rx.changed().await.is_err() {
                        // Sender dropped without setting to true — don't abort
                        std::future::pending::<()>().await;
                    }
                }
            } => {
                // Kill the child process by PID securely (ensure pid > 0 to prevent killing process group 0)
                if let Some(pid) = child_id {
                    if pid > 0 {
                        unsafe { libc::kill(pid as i32, libc::SIGKILL); }
                    }
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

                // Include sandbox info in output when sandboxed
                let result_data = if self.sandbox.is_some() {
                    json!({
                        "stdout": stdout.as_ref(),
                        "stderr": stderr.as_ref(),
                        "exit_code": exit_code,
                        "output": combined,
                        "sandbox": self.sandbox.as_ref().map(|s| s.description()).unwrap_or("none"),
                    })
                } else {
                    json!({
                        "stdout": stdout.as_ref(),
                        "stderr": stderr.as_ref(),
                        "exit_code": exit_code,
                        "output": combined,
                    })
                };

                Ok(ToolResult {
                    data: result_data,
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
        let (tx, rx) = tokio::sync::watch::channel(false);
        // Keep tx alive — dropping it would cause the abort future to resolve immediately
        std::mem::forget(tx);
        ToolContext {
            cwd: PathBuf::from("/tmp"),
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
            file_cache: None,
            tool_result_store: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
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

    // --- Sandbox integration tests ---

    #[test]
    fn test_build_command_no_sandbox() {
        let tool = BashTool::new();
        let (program, args) = tool.build_command("echo hello", Path::new("/tmp"));
        assert_eq!(program, "/bin/bash");
        assert_eq!(args, vec!["-c", "echo hello"]);
    }

    #[test]
    fn test_build_command_sandbox_none_backend() {
        let tool = BashTool::with_sandbox(Arc::new(SandboxConfig::default()));
        let (program, args) = tool.build_command("echo hello", Path::new("/tmp"));
        // Default backend is None → direct execution
        assert_eq!(program, "/bin/bash");
        assert_eq!(args, vec!["-c", "echo hello"]);
    }

    #[test]
    fn test_build_command_sandbox_docker() {
        let tool = BashTool::with_sandbox(Arc::new(SandboxConfig {
            backend: crate::engine::sandbox::SandboxBackend::Docker {
                image: "baoclaw-sandbox:latest".into(),
            },
            ..SandboxConfig::default()
        }));
        let (program, args) = tool.build_command("echo hello", Path::new("/tmp"));
        // Docker backend → direct docker execution (no shell wrapping)
        assert_eq!(program, "docker");
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"baoclaw-sandbox:latest".to_string()));
        // The user command is passed as the last arg to /bin/sh -c inside docker
        assert!(args.contains(&"echo hello".to_string()));
        assert!(args.contains(&"/bin/sh".to_string()));
        assert!(args.contains(&"-c".to_string()));
    }

    #[test]
    fn test_sandbox_prompt_includes_description() {
        let tool_no_sandbox = BashTool::new();
        assert!(!tool_no_sandbox.prompt().contains("sandbox"));

        let tool_with_sandbox = BashTool::with_sandbox(Arc::new(SandboxConfig {
            backend: crate::engine::sandbox::SandboxBackend::Docker {
                image: "baoclaw-sandbox:latest".into(),
            },
            ..SandboxConfig::default()
        }));
        assert!(tool_with_sandbox.prompt().contains("sandbox"));
        assert!(tool_with_sandbox.prompt().contains("Docker"));
    }

    #[tokio::test]
    async fn test_bash_sandbox_output_includes_sandbox_field() {
        // Only test the "sandbox: none" path (default config) to avoid needing Docker
        let tool = BashTool::with_sandbox(Arc::new(SandboxConfig::default()));
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"command": "echo sandbox_test"}), &ctx, &progress)
            .await
            .unwrap();

        assert!(!result.is_error);
        // When sandbox is set (even to None backend), output includes sandbox field
        assert!(result.data.get("sandbox").is_some());
    }
}
