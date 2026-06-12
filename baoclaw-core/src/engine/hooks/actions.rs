//! Action types and executor for the hook system.
//!
//! This module defines the actions that hooks can execute and the executor
//! that runs them.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use super::triggers::TriggerContext;

/// Types of actions that hooks can execute.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    /// Execute a shell command
    RunCommand,
    /// Send a prompt to the agent
    AskAgent,
    /// Send a notification
    SendNotification,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RunCommand => write!(f, "run_command"),
            Self::AskAgent => write!(f, "ask_agent"),
            Self::SendNotification => write!(f, "send_notification"),
        }
    }
}

impl std::str::FromStr for ActionType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "run_command" => Ok(Self::RunCommand),
            "ask_agent" => Ok(Self::AskAgent),
            "send_notification" => Ok(Self::SendNotification),
            _ => Err(format!("Unknown action type: {}", s)),
        }
    }
}

/// Action definition for a hook.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    /// The type of action to execute
    #[serde(rename = "type")]
    pub action_type: ActionType,

    /// Command to run (for run_command)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Prompt to send (for ask_agent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    /// Message to send (for send_notification)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Notification channels (for send_notification)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,

    /// Timeout in seconds for the action
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    30
}

impl Action {
    /// Create a run_command action.
    pub fn run_command(command: impl Into<String>) -> Self {
        Self {
            action_type: ActionType::RunCommand,
            command: Some(command.into()),
            prompt: None,
            message: None,
            channels: Vec::new(),
            timeout_secs: 30,
        }
    }

    /// Create an ask_agent action.
    pub fn ask_agent(prompt: impl Into<String>) -> Self {
        Self {
            action_type: ActionType::AskAgent,
            command: None,
            prompt: Some(prompt.into()),
            message: None,
            channels: Vec::new(),
            timeout_secs: 60,
        }
    }

    /// Create a send_notification action.
    pub fn send_notification(message: impl Into<String>) -> Self {
        Self {
            action_type: ActionType::SendNotification,
            command: None,
            prompt: None,
            message: Some(message.into()),
            channels: vec!["cli".to_string()],
            timeout_secs: 5,
        }
    }

    /// Set the timeout for this action.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set the notification channels.
    pub fn with_channels(mut self, channels: Vec<String>) -> Self {
        self.channels = channels;
        self
    }

    /// Substitute variables in a template string.
    pub fn substitute_variables(&self, template: &str, ctx: &TriggerContext) -> String {
        let mut result = template.to_string();

        // Find all {variable} patterns and substitute
        let mut start = 0;
        while let Some(open) = result[start..].find('{') {
            let open_idx = start + open;
            if let Some(close) = result[open_idx..].find('}') {
                let close_idx = open_idx + close;
                let var_name = &result[open_idx + 1..close_idx];

                if let Some(value) = ctx.get_variable(var_name) {
                    result = format!("{}{}{}", &result[..open_idx], value, &result[close_idx + 1..]);
                    // Continue from the same position since we modified the string
                    start = open_idx + value.len();
                } else {
                    // Variable not found, skip past this variable
                    start = close_idx + 1;
                }
            } else {
                break;
            }
        }

        result
    }
}

/// Result of an action execution.
#[derive(Clone, Debug)]
pub struct ActionResult {
    /// Whether the action succeeded
    pub success: bool,

    /// Output from the action (if any)
    pub output: Option<String>,

    /// Error message (if failed)
    pub error: Option<String>,
}

impl ActionResult {
    /// Create a successful result.
    pub fn success(output: Option<String>) -> Self {
        Self {
            success: true,
            output,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: None,
            error: Some(error.into()),
        }
    }
}

/// Pending action that needs to be processed by the main agent.
#[derive(Clone, Debug)]
pub struct PendingAction {
    /// The prompt to send to the agent (for ask_agent actions)
    pub prompt: String,

    /// The notification message (for send_notification actions)
    pub notification: Option<String>,

    /// The notification channels
    pub channels: Vec<String>,
}

/// Executor for hook actions.
pub struct ActionExecutor {
    /// Working directory for command execution
    cwd: PathBuf,

    /// Environment variables for command execution
    env: HashMap<String, String>,
}

impl ActionExecutor {
    /// Create a new action executor.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            env: HashMap::new(),
        }
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Execute an action.
    ///
    /// Returns the action result, or a pending action for actions that need
    /// to be processed by the main agent (ask_agent, send_notification).
    pub async fn execute(&self, action: &Action, ctx: &TriggerContext) -> (ActionResult, Option<PendingAction>) {
        match action.action_type {
            ActionType::RunCommand => self.execute_command(action, ctx).await,
            ActionType::AskAgent => self.prepare_ask_agent(action, ctx).await,
            ActionType::SendNotification => self.prepare_notification(action, ctx).await,
        }
    }

    async fn execute_command(&self, action: &Action, ctx: &TriggerContext) -> (ActionResult, Option<PendingAction>) {
        let command = match &action.command {
            Some(cmd) => cmd,
            None => return (ActionResult::failure("No command specified"), None),
        };

        // Substitute variables
        let command = action.substitute_variables(command, ctx);

        // Parse command into program and args
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return (ActionResult::failure("Empty command"), None);
        }

        let program = parts[0];
        let args: Vec<&str> = parts[1..].to_vec();

        // Build and execute command
        let mut cmd = Command::new(program);
        cmd.args(&args)
            .current_dir(&self.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Add environment variables
        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        let timeout_duration = Duration::from_secs(action.timeout_secs);

        match timeout(timeout_duration, cmd.output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    let result = if stdout.is_empty() {
                        ActionResult::success(None)
                    } else {
                        ActionResult::success(Some(stdout))
                    };
                    (result, None)
                } else {
                    (ActionResult::failure(format!("Command failed: {}", stderr)), None)
                }
            }
            Ok(Err(e)) => (ActionResult::failure(format!("Failed to execute command: {}", e)), None),
            Err(_) => (ActionResult::failure(format!("Command timed out after {}s", action.timeout_secs)), None),
        }
    }

    async fn prepare_ask_agent(&self, action: &Action, ctx: &TriggerContext) -> (ActionResult, Option<PendingAction>) {
        let prompt = match &action.prompt {
            Some(p) => p,
            None => return (ActionResult::failure("No prompt specified"), None),
        };

        let prompt = action.substitute_variables(prompt, ctx);

        (
            ActionResult::success(Some("Pending agent prompt".to_string())),
            Some(PendingAction {
                prompt,
                notification: None,
                channels: Vec::new(),
            }),
        )
    }

    async fn prepare_notification(&self, action: &Action, ctx: &TriggerContext) -> (ActionResult, Option<PendingAction>) {
        let message = match &action.message {
            Some(m) => m,
            None => return (ActionResult::failure("No message specified"), None),
        };

        let message = action.substitute_variables(message, ctx);

        (
            ActionResult::success(Some("Pending notification".to_string())),
            Some(PendingAction {
                prompt: String::new(),
                notification: Some(message),
                channels: action.channels.clone(),
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_action_type_from_str() {
        assert_eq!(
            ActionType::from_str("run_command").unwrap(),
            ActionType::RunCommand
        );
        assert_eq!(
            ActionType::from_str("ask_agent").unwrap(),
            ActionType::AskAgent
        );
        assert_eq!(
            ActionType::from_str("send_notification").unwrap(),
            ActionType::SendNotification
        );
        assert!(ActionType::from_str("invalid").is_err());
    }

    #[test]
    fn test_action_type_display() {
        assert_eq!(ActionType::RunCommand.to_string(), "run_command");
        assert_eq!(ActionType::AskAgent.to_string(), "ask_agent");
        assert_eq!(ActionType::SendNotification.to_string(), "send_notification");
    }

    #[test]
    fn test_action_run_command_constructor() {
        let action = Action::run_command("npm test");
        assert_eq!(action.action_type, ActionType::RunCommand);
        assert_eq!(action.command, Some("npm test".to_string()));
        assert_eq!(action.timeout_secs, 30);
        assert!(action.prompt.is_none());
        assert!(action.message.is_none());
    }

    #[test]
    fn test_action_ask_agent_constructor() {
        let action = Action::ask_agent("Review this code");
        assert_eq!(action.action_type, ActionType::AskAgent);
        assert_eq!(action.prompt, Some("Review this code".to_string()));
        assert_eq!(action.timeout_secs, 60);
        assert!(action.command.is_none());
        assert!(action.message.is_none());
    }

    #[test]
    fn test_action_send_notification_constructor() {
        let action = Action::send_notification("Build completed");
        assert_eq!(action.action_type, ActionType::SendNotification);
        assert_eq!(action.message, Some("Build completed".to_string()));
        assert_eq!(action.channels, vec!["cli".to_string()]);
        assert_eq!(action.timeout_secs, 5);
    }

    #[test]
    fn test_action_with_timeout() {
        let action = Action::run_command("npm test").with_timeout(120);
        assert_eq!(action.timeout_secs, 120);
    }

    #[test]
    fn test_action_with_channels() {
        let action = Action::send_notification("Test failed")
            .with_channels(vec!["telegram".to_string(), "email".to_string()]);
        assert_eq!(action.channels, vec!["telegram", "email"]);
    }

    #[test]
    fn test_action_substitute_variables_file() {
        let action = Action::run_command("echo {file}");
        let ctx = TriggerContext::file_edited("src/main.ts", "/project");

        let result = action.substitute_variables("echo {file}", &ctx);
        assert_eq!(result, "echo src/main.ts");
    }

    #[test]
    fn test_action_substitute_variables_cwd() {
        let action = Action::run_command("cd {cwd}");
        let ctx = TriggerContext::file_edited("src/main.ts", "/project");

        let result = action.substitute_variables("cd {cwd}", &ctx);
        assert_eq!(result, "cd /project");
    }

    #[test]
    fn test_action_substitute_variables_multiple() {
        let action = Action::run_command("echo {file} in {cwd}");
        let ctx = TriggerContext::file_edited("src/main.ts", "/project");

        let result = action.substitute_variables("echo {file} in {cwd}", &ctx);
        assert_eq!(result, "echo src/main.ts in /project");
    }

    #[test]
    fn test_action_substitute_variables_tool_context() {
        let action = Action::run_command("echo {tool}: {output}");
        let ctx = TriggerContext::tool_result("Bash", "npm test", "all tests passed");

        let result = action.substitute_variables("echo {tool}: {output}", &ctx);
        assert_eq!(result, "echo Bash: all tests passed");
    }

    #[test]
    fn test_action_substitute_variables_error_context() {
        let action = Action::send_notification("Error: {error}");
        let ctx = TriggerContext::error("Build failed", Some("Bash".to_string()));

        let result = action.substitute_variables("Error: {error}", &ctx);
        assert_eq!(result, "Error: Build failed");
    }

    #[test]
    fn test_action_substitute_variables_session_context() {
        let action = Action::send_notification("Session {session_id}: {turns} turns, ${cost}");
        let ctx = TriggerContext::session_end("abc123", 42, 5.25);

        let result = action.substitute_variables("Session {session_id}: {turns} turns, ${cost}", &ctx);
        assert_eq!(result, "Session abc123: 42 turns, $5.25");
    }

    #[test]
    fn test_action_substitute_variables_unknown_variable() {
        let action = Action::run_command("echo {unknown}");
        let ctx = TriggerContext::new();

        let result = action.substitute_variables("echo {unknown}", &ctx);
        assert_eq!(result, "echo {unknown}");
    }

    #[test]
    fn test_action_result_success() {
        let result = ActionResult::success(Some("output".to_string()));
        assert!(result.success);
        assert_eq!(result.output, Some("output".to_string()));
        assert!(result.error.is_none());
    }

    #[test]
    fn test_action_result_success_no_output() {
        let result = ActionResult::success(None);
        assert!(result.success);
        assert!(result.output.is_none());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_action_result_failure() {
        let result = ActionResult::failure("error message");
        assert!(!result.success);
        assert!(result.output.is_none());
        assert_eq!(result.error, Some("error message".to_string()));
    }

    #[test]
    fn test_pending_action_ask_agent() {
        let pending = PendingAction {
            prompt: "Review this code".to_string(),
            notification: None,
            channels: Vec::new(),
        };
        assert_eq!(pending.prompt, "Review this code");
        assert!(pending.notification.is_none());
    }

    #[test]
    fn test_pending_action_notification() {
        let pending = PendingAction {
            prompt: String::new(),
            notification: Some("Build failed".to_string()),
            channels: vec!["telegram".to_string()],
        };
        assert!(pending.prompt.is_empty());
        assert_eq!(pending.notification, Some("Build failed".to_string()));
        assert_eq!(pending.channels, vec!["telegram"]);
    }

    // ========================================
    // ActionExecutor Tests
    // ========================================

    #[tokio::test]
    async fn test_executor_new() {
        let executor = ActionExecutor::new("/tmp");
        assert!(executor.env.is_empty());
    }

    #[tokio::test]
    async fn test_executor_with_env() {
        let executor = ActionExecutor::new("/tmp")
            .with_env("PATH", "/usr/bin")
            .with_env("HOME", "/home/user");
        assert_eq!(executor.env.get("PATH"), Some(&"/usr/bin".to_string()));
        assert_eq!(executor.env.get("HOME"), Some(&"/home/user".to_string()));
    }

    #[tokio::test]
    async fn test_execute_command_success() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action::run_command("echo hello");
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        assert!(pending.is_none());
        assert!(result.output.unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn test_execute_command_with_variable_substitution() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action::run_command("echo {file}");
        let ctx = TriggerContext::file_edited("test.txt", "/tmp");

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        assert!(pending.is_none());
        assert!(result.output.unwrap().contains("test.txt"));
    }

    #[tokio::test]
    async fn test_execute_command_failure() {
        let executor = ActionExecutor::new("/tmp");
        // Use a command that will fail
        let action = Action::run_command("ls /nonexistent_directory_12345");
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(!result.success);
        assert!(pending.is_none());
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_execute_command_empty_command() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action {
            action_type: ActionType::RunCommand,
            command: None,
            prompt: None,
            message: None,
            channels: Vec::new(),
            timeout_secs: 30,
        };
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(!result.success);
        assert!(pending.is_none());
        assert!(result.error.unwrap().contains("No command specified"));
    }

    #[tokio::test]
    async fn test_execute_command_whitespace_only() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action::run_command("   ");
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(!result.success);
        assert!(pending.is_none());
        assert!(result.error.unwrap().contains("Empty command"));
    }

    #[tokio::test]
    async fn test_execute_command_timeout() {
        let executor = ActionExecutor::new("/tmp");
        // A command that sleeps for longer than the timeout
        let action = Action::run_command("sleep 10").with_timeout(1);
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(!result.success);
        assert!(pending.is_none());
        assert!(result.error.unwrap().contains("timed out"));
    }

    #[tokio::test]
    async fn test_execute_command_with_env() {
        let executor = ActionExecutor::new("/tmp")
            .with_env("MY_VAR", "test_value");
        // Use env command to verify environment variable is set
        let action = Action::run_command("env");
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success, "Command should succeed, error: {:?}", result.error);
        assert!(pending.is_none());
        let output = result.output.unwrap();
        assert!(output.contains("MY_VAR=test_value"), "Output should contain MY_VAR=test_value, got partial: {}", &output[..output.len().min(500)]);
    }

    #[tokio::test]
    async fn test_prepare_ask_agent_success() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action::ask_agent("Review {file}");
        let ctx = TriggerContext::file_edited("src/main.ts", "/project");

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        assert!(pending.is_some());
        let pending = pending.unwrap();
        assert_eq!(pending.prompt, "Review src/main.ts");
        assert!(pending.notification.is_none());
        assert!(pending.channels.is_empty());
    }

    #[tokio::test]
    async fn test_prepare_ask_agent_with_session_context() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action::ask_agent("Session {session_id} had {turns} turns");
        let ctx = TriggerContext::session_end("abc123", 10, 2.5);

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        let pending = pending.unwrap();
        assert_eq!(pending.prompt, "Session abc123 had 10 turns");
    }

    #[tokio::test]
    async fn test_prepare_ask_agent_no_prompt() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action {
            action_type: ActionType::AskAgent,
            command: None,
            prompt: None,
            message: None,
            channels: Vec::new(),
            timeout_secs: 60,
        };
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(!result.success);
        assert!(pending.is_none());
        assert!(result.error.unwrap().contains("No prompt specified"));
    }

    #[tokio::test]
    async fn test_prepare_notification_success() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action::send_notification("File {file} was edited")
            .with_channels(vec!["telegram".to_string(), "cli".to_string()]);
        let ctx = TriggerContext::file_edited("src/main.ts", "/project");

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        assert!(pending.is_some());
        let pending = pending.unwrap();
        assert!(pending.prompt.is_empty());
        assert_eq!(pending.notification, Some("File src/main.ts was edited".to_string()));
        assert_eq!(pending.channels, vec!["telegram", "cli"]);
    }

    #[tokio::test]
    async fn test_prepare_notification_with_error_context() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action::send_notification("Error: {error}")
            .with_channels(vec!["email".to_string()]);
        let ctx = TriggerContext::error("Build failed", Some("Bash".to_string()));

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        let pending = pending.unwrap();
        assert_eq!(pending.notification, Some("Error: Build failed".to_string()));
    }

    #[tokio::test]
    async fn test_prepare_notification_no_message() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action {
            action_type: ActionType::SendNotification,
            command: None,
            prompt: None,
            message: None,
            channels: vec!["cli".to_string()],
            timeout_secs: 5,
        };
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(!result.success);
        assert!(pending.is_none());
        assert!(result.error.unwrap().contains("No message specified"));
    }

    #[tokio::test]
    async fn test_prepare_notification_default_channels() {
        let executor = ActionExecutor::new("/tmp");
        let action = Action::send_notification("Test notification");
        let ctx = TriggerContext::new();

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        let pending = pending.unwrap();
        assert_eq!(pending.channels, vec!["cli"]);
    }

    // ========================================
    // Integration Tests
    // ========================================

    #[tokio::test]
    async fn test_full_workflow_run_command() {
        // Simulate a full workflow: file edited -> run lint
        let executor = ActionExecutor::new("/tmp");
        let action = Action::run_command("echo 'Linting {file}'").with_timeout(10);
        let ctx = TriggerContext::file_edited("src/lib.rs", "/project");

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        assert!(pending.is_none());
        assert!(result.output.unwrap().contains("Linting src/lib.rs"));
    }

    #[tokio::test]
    async fn test_full_workflow_ask_agent() {
        // Simulate: error occurred -> ask agent for help
        let executor = ActionExecutor::new("/tmp");
        let action = Action::ask_agent("Analyze this error: {error}").with_timeout(30);
        let ctx = TriggerContext::error("undefined variable 'x'", Some("Bash".to_string()));

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        let pending = pending.unwrap();
        assert_eq!(pending.prompt, "Analyze this error: undefined variable 'x'");
    }

    #[tokio::test]
    async fn test_full_workflow_send_notification() {
        // Simulate: session ended -> send notification
        let executor = ActionExecutor::new("/tmp");
        let action = Action::send_notification(
            "Session complete: {turns} turns, cost ${cost}"
        ).with_channels(vec!["telegram".to_string()]);
        let ctx = TriggerContext::session_end("sess-001", 25, 3.50);

        let (result, pending) = executor.execute(&action, &ctx).await;
        assert!(result.success);
        let pending = pending.unwrap();
        // The cost will be formatted as "3.5" since Rust's Display for f64 doesn't force trailing zeros
        assert!(pending.notification.as_ref().unwrap().contains("25 turns"));
        assert!(pending.notification.as_ref().unwrap().contains("$3"));
    }
}
