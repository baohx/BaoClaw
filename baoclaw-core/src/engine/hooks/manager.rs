//! Hook manager for registration, loading, and execution.
//!
//! This module provides the main hook manager that coordinates hook
//! registration, loading from configuration, and execution.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

use super::actions::{Action, ActionExecutor, ActionResult, PendingAction};
use super::triggers::{Filter, TriggerContext, TriggerType};

/// A hook definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hook {
    /// Unique identifier for this hook
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// The trigger that activates this hook
    pub trigger: TriggerType,

    /// Optional filter conditions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,

    /// The action to execute when triggered
    pub action: Action,

    /// Whether this hook is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Priority for execution order (higher = earlier)
    #[serde(default)]
    pub priority: i32,
}

fn default_enabled() -> bool {
    true
}

impl Hook {
    /// Create a new hook with the given ID, trigger, and action.
    pub fn new(id: impl Into<String>, name: impl Into<String>, trigger: TriggerType, action: Action) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            trigger,
            filter: None,
            action,
            enabled: true,
            priority: 0,
        }
    }

    /// Add a filter to this hook.
    pub fn with_filter(mut self, filter: Filter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Set the enabled state.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Check if this hook matches the given trigger type and context.
    pub fn matches(&self, trigger_type: &TriggerType, ctx: &TriggerContext) -> bool {
        if !self.enabled {
            return false;
        }

        if &self.trigger != trigger_type {
            return false;
        }

        if let Some(filter) = &self.filter {
            // Check file filter
            if let Some(file) = &ctx.file {
                if !filter.matches_file(file) {
                    return false;
                }
            }

            // Check tool filter
            if let Some(tool) = &ctx.tool {
                if !filter.matches_tool(tool) {
                    return false;
                }
            }

            // Check regex filter
            let regex_text = match &self.trigger {
                TriggerType::ToolResult => ctx.tool_input.as_ref().or(ctx.tool_output.as_ref()),
                TriggerType::UserMessage => ctx.user_message.as_ref(),
                TriggerType::AssistantMessage => ctx.assistant_message.as_ref(),
                TriggerType::Error => ctx.error.as_ref(),
                _ => None,
            };

            if let Some(text) = regex_text {
                if !filter.matches_regex(text) {
                    return false;
                }
            }
        }

        true
    }
}

/// Configuration for the hook manager.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct HookManagerConfig {
    /// List of hooks to manage
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<Hook>,

    /// Maximum concurrent hook executions
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// Whether to continue on errors
    #[serde(default = "default_continue_on_error")]
    pub continue_on_error: bool,
}

fn default_max_concurrent() -> usize {
    5
}

fn default_continue_on_error() -> bool {
    true
}

impl HookManagerConfig {
    /// Load configuration from a file.
    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                match serde_json::from_str(&content) {
                    Ok(config) => config,
                    Err(e) => {
                        eprintln!("Failed to parse hooks config: {}", e);
                        Self::default()
                    }
                }
            }
            Err(_) => Self::default(),
        }
    }

    /// Save configuration to a file.
    pub fn save(&self, path: &PathBuf) -> Result<(), std::io::Error> {
        let content = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }
}

/// Hook configuration file format.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    pub hooks: Vec<Hook>,
}

/// Result of processing hooks.
#[derive(Debug, Default)]
pub struct HookProcessingResult {
    /// Number of hooks that were triggered
    pub triggered_count: usize,

    /// Number of hooks that succeeded
    pub success_count: usize,

    /// Number of hooks that failed
    pub failure_count: usize,

    /// Pending actions that need to be processed by the main agent
    pub pending_actions: Vec<PendingAction>,

    /// Errors that occurred during execution
    pub errors: Vec<String>,
}

/// Statistics for hook execution.
#[derive(Clone, Debug, Default)]
pub struct HookStats {
    /// Total number of hook executions
    pub total_executions: u64,

    /// Number of successful executions
    pub successful_executions: u64,

    /// Number of failed executions
    pub failed_executions: u64,

    /// Total execution time in milliseconds
    pub total_execution_time_ms: u64,

    /// Executions per hook ID
    pub executions_by_hook: HashMap<String, u64>,
}

/// Manager for hooks.
pub struct HookManager {
    /// Configuration
    config: RwLock<HookManagerConfig>,

    /// Configuration file path
    config_path: PathBuf,

    /// Action executor
    executor: RwLock<Option<ActionExecutor>>,

    /// Execution statistics
    stats: RwLock<HookStats>,
}

impl HookManager {
    /// Create a new hook manager with default configuration.
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let config_path = PathBuf::from(&home).join(".baoclaw").join("hooks.json");

        let config = HookManagerConfig::load(&config_path);
        if !config.hooks.is_empty() {
            eprintln!("Loaded {} hooks from {}", config.hooks.len(), config_path.display());
        }

        Self {
            config: RwLock::new(config),
            config_path,
            executor: RwLock::new(None),
            stats: RwLock::new(HookStats::default()),
        }
    }

    /// Create a hook manager with a custom config path.
    pub fn with_config_path(config_path: impl Into<PathBuf>) -> Self {
        let config_path = config_path.into();
        let config = HookManagerConfig::load(&config_path);

        Self {
            config: RwLock::new(config),
            config_path,
            executor: RwLock::new(None),
            stats: RwLock::new(HookStats::default()),
        }
    }

    /// Set the working directory for the action executor.
    pub async fn set_working_directory(&self, cwd: impl Into<PathBuf>) {
        let mut executor = self.executor.write().await;
        *executor = Some(ActionExecutor::new(cwd));
    }

    /// Get all hooks.
    pub async fn get_hooks(&self) -> Vec<Hook> {
        self.config.read().await.hooks.clone()
    }

    /// Get a hook by ID.
    pub async fn get_hook(&self, id: &str) -> Option<Hook> {
        self.config.read().await.hooks.iter().find(|h| h.id == id).cloned()
    }

    /// Add a hook.
    pub async fn add_hook(&self, hook: Hook) -> Result<(), String> {
        let mut config = self.config.write().await;

        // Check for duplicate ID
        if config.hooks.iter().any(|h| h.id == hook.id) {
            return Err(format!("Hook with ID '{}' already exists", hook.id));
        }

        config.hooks.push(hook);

        // Save to file
        if let Err(e) = config.save(&self.config_path) {
            eprintln!("Failed to save hooks config: {}", e);
        }

        Ok(())
    }

    /// Update a hook.
    pub async fn update_hook(&self, hook: Hook) -> Result<(), String> {
        let mut config = self.config.write().await;

        if let Some(existing) = config.hooks.iter_mut().find(|h| h.id == hook.id) {
            *existing = hook;
        } else {
            return Err(format!("Hook with ID '{}' not found", hook.id));
        }

        // Save to file
        if let Err(e) = config.save(&self.config_path) {
            eprintln!("Failed to save hooks config: {}", e);
        }

        Ok(())
    }

    /// Remove a hook by ID.
    pub async fn remove_hook(&self, id: &str) -> bool {
        let mut config = self.config.write().await;
        let before = config.hooks.len();
        config.hooks.retain(|h| h.id != id);

        if config.hooks.len() < before {
            // Save to file
            if let Err(e) = config.save(&self.config_path) {
                eprintln!("Failed to save hooks config: {}", e);
            }
            true
        } else {
            false
        }
    }

    /// Enable a hook by ID.
    pub async fn enable_hook(&self, id: &str) -> bool {
        let mut config = self.config.write().await;
        if let Some(hook) = config.hooks.iter_mut().find(|h| h.id == id) {
            hook.enabled = true;
            if let Err(e) = config.save(&self.config_path) {
                eprintln!("Failed to save hooks config: {}", e);
            }
            true
        } else {
            false
        }
    }

    /// Disable a hook by ID.
    pub async fn disable_hook(&self, id: &str) -> bool {
        let mut config = self.config.write().await;
        if let Some(hook) = config.hooks.iter_mut().find(|h| h.id == id) {
            hook.enabled = false;
            if let Err(e) = config.save(&self.config_path) {
                eprintln!("Failed to save hooks config: {}", e);
            }
            true
        } else {
            false
        }
    }

    /// Toggle a hook's enabled state by ID.
    /// Returns the new enabled state, or None if the hook was not found.
    pub async fn toggle_hook(&self, id: &str) -> Option<bool> {
        let mut config = self.config.write().await;
        if let Some(hook) = config.hooks.iter_mut().find(|h| h.id == id) {
            hook.enabled = !hook.enabled;
            let new_state = hook.enabled;
            if let Err(e) = config.save(&self.config_path) {
                eprintln!("Failed to save hooks config: {}", e);
            }
            Some(new_state)
        } else {
            None
        }
    }

    /// Process hooks for a given trigger type and context.
    /// Returns the result of processing.
    pub async fn process(&self, trigger_type: TriggerType, ctx: TriggerContext) -> HookProcessingResult {
        let config = self.config.read().await;
        let executor = self.executor.read().await;

        // Find matching hooks, sorted by priority (descending)
        let mut matching_hooks: Vec<&Hook> = config
            .hooks
            .iter()
            .filter(|h| h.matches(&trigger_type, &ctx))
            .collect();
        matching_hooks.sort_by_key(|a| std::cmp::Reverse(a.priority));

        let mut result = HookProcessingResult::default();

        for hook in matching_hooks {
            result.triggered_count += 1;
            let start = std::time::Instant::now();

            // Execute the action
            let (action_result, pending) = if let Some(exec) = executor.as_ref() {
                exec.execute(&hook.action, &ctx).await
            } else {
                // No executor, try to create pending actions for ask_agent/send_notification
                match &hook.action.action_type {
                    super::actions::ActionType::AskAgent => {
                        let prompt = hook.action.prompt.clone().unwrap_or_default();
                        let prompt = hook.action.substitute_variables(&prompt, &ctx);
                        (
                            ActionResult::success(Some("Pending agent prompt".to_string())),
                            Some(PendingAction {
                                prompt,
                                notification: None,
                                channels: Vec::new(),
                            }),
                        )
                    }
                    super::actions::ActionType::SendNotification => {
                        let message = hook.action.message.clone().unwrap_or_default();
                        let message = hook.action.substitute_variables(&message, &ctx);
                        (
                            ActionResult::success(Some("Pending notification".to_string())),
                            Some(PendingAction {
                                prompt: String::new(),
                                notification: Some(message),
                                channels: hook.action.channels.clone(),
                            }),
                        )
                    }
                    _ => (ActionResult::failure("No executor available"), None),
                }
            };

            // Update statistics
            {
                let mut stats = self.stats.write().await;
                stats.total_executions += 1;
                stats.total_execution_time_ms += start.elapsed().as_millis() as u64;
                *stats.executions_by_hook.entry(hook.id.clone()).or_insert(0) += 1;

                if action_result.success {
                    stats.successful_executions += 1;
                    result.success_count += 1;
                } else {
                    stats.failed_executions += 1;
                    result.failure_count += 1;
                    if let Some(error) = action_result.error {
                        result.errors.push(format!("Hook '{}': {}", hook.id, error));
                    }
                }
            }

            // Collect pending actions
            if let Some(pending) = pending {
                result.pending_actions.push(pending);
            }
        }

        result
    }

    /// Get execution statistics.
    pub async fn get_stats(&self) -> HookStats {
        self.stats.read().await.clone()
    }

    /// Reset execution statistics.
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        *stats = HookStats::default();
    }

    /// Reload configuration from file.
    pub async fn reload(&self) {
        let config = HookManagerConfig::load(&self.config_path);
        *self.config.write().await = config;
    }
}

impl Default for HookManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::actions::ActionType;

    #[test]
    fn test_hook_new() {
        let hook = Hook::new(
            "test-hook",
            "Test Hook",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        );

        assert_eq!(hook.id, "test-hook");
        assert_eq!(hook.name, "Test Hook");
        assert_eq!(hook.trigger, TriggerType::FileEdited);
        assert!(hook.enabled);
    }

    #[test]
    fn test_hook_with_filter() {
        let hook = Hook::new(
            "test-hook",
            "Test Hook",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        )
        .with_filter(Filter::file_pattern("*.ts"));

        assert!(hook.filter.is_some());
    }

    #[test]
    fn test_hook_matches() {
        let hook = Hook::new(
            "test-hook",
            "Test Hook",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        )
        .with_filter(Filter::file_pattern("*.ts"));

        let ctx = TriggerContext::file_edited("src/main.ts", "/project");
        assert!(hook.matches(&TriggerType::FileEdited, &ctx));

        let ctx = TriggerContext::file_edited("src/main.rs", "/project");
        assert!(!hook.matches(&TriggerType::FileEdited, &ctx));
    }

    #[tokio::test]
    async fn test_hook_manager_add_hook() {
        // Use a unique test file in /tmp
        let test_path = format!("/tmp/test-hooks-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        let hook = Hook::new(
            "test-hook",
            "Test Hook",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        );

        let result = manager.add_hook(hook).await;
        assert!(result.is_ok(), "Failed to add hook: {:?}", result.err());

        // Duplicate should fail
        let hook = Hook::new(
            "test-hook",
            "Test Hook 2",
            TriggerType::FileEdited,
            Action::run_command("echo test2"),
        );
        assert!(manager.add_hook(hook).await.is_err());

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[test]
    fn test_hook_json_deserialization() {
        // Test that hooks can be deserialized from JSON matching the design document format
        let json = r#"{
            "id": "auto-lint-on-save",
            "name": "Auto Lint on Save",
            "trigger": "file_edited",
            "filter": {
                "file_pattern": "*.ts"
            },
            "action": {
                "type": "run_command",
                "command": "npm run lint --fix {file}",
                "timeout_secs": 30
            },
            "enabled": true,
            "priority": 100
        }"#;

        let hook: Hook = serde_json::from_str(json).expect("Failed to deserialize hook");
        assert_eq!(hook.id, "auto-lint-on-save");
        assert_eq!(hook.name, "Auto Lint on Save");
        assert_eq!(hook.trigger, TriggerType::FileEdited);
        assert!(hook.enabled);
        assert_eq!(hook.priority, 100);

        // Verify filter
        let filter = hook.filter.expect("Hook should have filter");
        assert_eq!(filter.file_pattern, Some("*.ts".to_string()));

        // Verify action
        assert_eq!(hook.action.action_type, ActionType::RunCommand);
        assert_eq!(hook.action.command, Some("npm run lint --fix {file}".to_string()));
        assert_eq!(hook.action.timeout_secs, 30);
    }

    #[test]
    fn test_hooks_config_json_deserialization() {
        // Test that the full config format matches the design document
        let json = r#"{
            "hooks": [
                {
                    "id": "auto-lint-on-save",
                    "name": "Auto Lint on Save",
                    "trigger": "file_edited",
                    "filter": {
                        "file_pattern": "*.ts"
                    },
                    "action": {
                        "type": "run_command",
                        "command": "npm run lint --fix {file}",
                        "timeout_secs": 30
                    },
                    "enabled": true,
                    "priority": 100
                },
                {
                    "id": "test-after-commit",
                    "name": "Run Tests After Commit",
                    "trigger": "tool_result",
                    "filter": {
                        "tool_name": "Bash",
                        "regex": "git commit"
                    },
                    "action": {
                        "type": "run_command",
                        "command": "npm test",
                        "timeout_secs": 120
                    },
                    "enabled": true
                },
                {
                    "id": "notify-on-error",
                    "name": "Notify on Error",
                    "trigger": "error",
                    "action": {
                        "type": "send_notification",
                        "message": "Error occurred: {error}",
                        "channels": ["telegram"]
                    }
                }
            ]
        }"#;

        let config: HookManagerConfig = serde_json::from_str(json).expect("Failed to deserialize config");
        assert_eq!(config.hooks.len(), 3);

        // Verify first hook
        let hook1 = &config.hooks[0];
        assert_eq!(hook1.id, "auto-lint-on-save");
        assert_eq!(hook1.trigger, TriggerType::FileEdited);
        assert_eq!(hook1.priority, 100);

        // Verify second hook
        let hook2 = &config.hooks[1];
        assert_eq!(hook2.id, "test-after-commit");
        assert_eq!(hook2.trigger, TriggerType::ToolResult);
        let filter2 = hook2.filter.as_ref().expect("Hook2 should have filter");
        assert_eq!(filter2.tool_name, Some("Bash".to_string()));
        assert_eq!(filter2.regex, Some("git commit".to_string()));

        // Verify third hook (no enabled field, should default to true)
        let hook3 = &config.hooks[2];
        assert_eq!(hook3.id, "notify-on-error");
        assert_eq!(hook3.trigger, TriggerType::Error);
        assert!(hook3.enabled); // Should default to true
    }

    #[test]
    fn test_hook_all_trigger_types() {
        // Test all trigger types from the design document
        let trigger_types = [
            ("file_edited", TriggerType::FileEdited),
            ("file_created", TriggerType::FileCreated),
            ("file_deleted", TriggerType::FileDeleted),
            ("tool_result", TriggerType::ToolResult),
            ("session_start", TriggerType::SessionStart),
            ("session_end", TriggerType::SessionEnd),
            ("user_message", TriggerType::UserMessage),
            ("assistant_message", TriggerType::AssistantMessage),
            ("error", TriggerType::Error),
        ];

        for (json_str, expected) in trigger_types {
            let json = format!(r#"{{"id":"test","name":"Test","trigger":"{}","action":{{"type":"run_command","command":"echo"}}}}"#, json_str);
            let hook: Hook = serde_json::from_str(&json).expect(&format!("Failed to parse trigger: {}", json_str));
            assert_eq!(hook.trigger, expected, "Trigger type mismatch for: {}", json_str);
        }
    }

    #[test]
    fn test_hook_all_action_types() {
        // Test run_command action
        let json = r#"{"id":"test","name":"Test","trigger":"file_edited","action":{"type":"run_command","command":"echo test","timeout_secs":60}}"#;
        let hook: Hook = serde_json::from_str(json).expect("Failed to parse run_command");
        assert_eq!(hook.action.action_type, ActionType::RunCommand);
        assert_eq!(hook.action.command, Some("echo test".to_string()));
        assert_eq!(hook.action.timeout_secs, 60);

        // Test ask_agent action
        let json = r#"{"id":"test","name":"Test","trigger":"file_edited","action":{"type":"ask_agent","prompt":"Review this code"}}"#;
        let hook: Hook = serde_json::from_str(json).expect("Failed to parse ask_agent");
        assert_eq!(hook.action.action_type, ActionType::AskAgent);
        assert_eq!(hook.action.prompt, Some("Review this code".to_string()));

        // Test send_notification action
        let json = r#"{"id":"test","name":"Test","trigger":"error","action":{"type":"send_notification","message":"Error: {error}","channels":["telegram","cli"]}}"#;
        let hook: Hook = serde_json::from_str(json).expect("Failed to parse send_notification");
        assert_eq!(hook.action.action_type, ActionType::SendNotification);
        assert_eq!(hook.action.message, Some("Error: {error}".to_string()));
        assert_eq!(hook.action.channels, vec!["telegram", "cli"]);
    }

    #[test]
    fn test_hook_config_save_and_load() {
        // Create a test config
        let config = HookManagerConfig {
            hooks: vec![
                Hook::new("hook1", "Hook 1", TriggerType::FileEdited, Action::run_command("cmd1"))
                    .with_filter(Filter::file_pattern("*.ts"))
                    .with_priority(100),
                Hook::new("hook2", "Hook 2", TriggerType::ToolResult, Action::ask_agent("prompt")),
            ],
            max_concurrent: 3,
            continue_on_error: true,
        };

        // Save to temp file
        let temp_path = std::env::temp_dir().join(format!("test-hooks-config-{}.json", uuid::Uuid::new_v4()));
        config.save(&temp_path).expect("Failed to save config");

        // Load from file
        let loaded = HookManagerConfig::load(&temp_path);
        assert_eq!(loaded.hooks.len(), 2);
        assert_eq!(loaded.hooks[0].id, "hook1");
        assert_eq!(loaded.hooks[1].id, "hook2");
        assert_eq!(loaded.max_concurrent, 3);

        // Cleanup
        let _ = std::fs::remove_file(&temp_path);
    }

    // ========================================
    // HookManager method tests (Task 1.3)
    // ========================================

    #[tokio::test]
    async fn test_remove_hook() {
        let test_path = format!("/tmp/test-hooks-remove-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add a hook
        let hook = Hook::new(
            "hook-to-remove",
            "Hook to Remove",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        );
        manager.add_hook(hook).await.expect("Failed to add hook");

        // Verify it was added
        let hooks = manager.get_hooks().await;
        assert_eq!(hooks.len(), 1);

        // Remove the hook
        let removed = manager.remove_hook("hook-to-remove").await;
        assert!(removed, "remove_hook should return true when hook exists");

        // Verify it was removed
        let hooks = manager.get_hooks().await;
        assert!(hooks.is_empty(), "Hook should be removed");

        // Try to remove non-existent hook
        let removed = manager.remove_hook("non-existent").await;
        assert!(!removed, "remove_hook should return false when hook doesn't exist");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_enable_hook() {
        let test_path = format!("/tmp/test-hooks-enable-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add a disabled hook
        let hook = Hook::new(
            "disabled-hook",
            "Disabled Hook",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        ).with_enabled(false);
        manager.add_hook(hook).await.expect("Failed to add hook");

        // Verify it's disabled
        let hook = manager.get_hook("disabled-hook").await.expect("Hook should exist");
        assert!(!hook.enabled, "Hook should be disabled");

        // Enable the hook
        let enabled = manager.enable_hook("disabled-hook").await;
        assert!(enabled, "enable_hook should return true when hook exists");

        // Verify it's enabled
        let hook = manager.get_hook("disabled-hook").await.expect("Hook should exist");
        assert!(hook.enabled, "Hook should now be enabled");

        // Try to enable non-existent hook
        let enabled = manager.enable_hook("non-existent").await;
        assert!(!enabled, "enable_hook should return false when hook doesn't exist");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_disable_hook() {
        let test_path = format!("/tmp/test-hooks-disable-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add an enabled hook
        let hook = Hook::new(
            "enabled-hook",
            "Enabled Hook",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        );
        manager.add_hook(hook).await.expect("Failed to add hook");

        // Verify it's enabled
        let hook = manager.get_hook("enabled-hook").await.expect("Hook should exist");
        assert!(hook.enabled, "Hook should be enabled");

        // Disable the hook
        let disabled = manager.disable_hook("enabled-hook").await;
        assert!(disabled, "disable_hook should return true when hook exists");

        // Verify it's disabled
        let hook = manager.get_hook("enabled-hook").await.expect("Hook should exist");
        assert!(!hook.enabled, "Hook should now be disabled");

        // Try to disable non-existent hook
        let disabled = manager.disable_hook("non-existent").await;
        assert!(!disabled, "disable_hook should return false when hook doesn't exist");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_toggle_hook() {
        let test_path = format!("/tmp/test-hooks-toggle-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add an enabled hook
        let hook = Hook::new(
            "toggle-hook",
            "Toggle Hook",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        );
        manager.add_hook(hook).await.expect("Failed to add hook");

        // Verify it's enabled
        let hook = manager.get_hook("toggle-hook").await.expect("Hook should exist");
        assert!(hook.enabled, "Hook should start enabled");

        // Toggle to disabled
        let new_state = manager.toggle_hook("toggle-hook").await;
        assert_eq!(new_state, Some(false), "toggle_hook should return new state (false)");
        let hook = manager.get_hook("toggle-hook").await.expect("Hook should exist");
        assert!(!hook.enabled, "Hook should be disabled after toggle");

        // Toggle back to enabled
        let new_state = manager.toggle_hook("toggle-hook").await;
        assert_eq!(new_state, Some(true), "toggle_hook should return new state (true)");
        let hook = manager.get_hook("toggle-hook").await.expect("Hook should exist");
        assert!(hook.enabled, "Hook should be enabled after second toggle");

        // Try to toggle non-existent hook
        let new_state = manager.toggle_hook("non-existent").await;
        assert_eq!(new_state, None, "toggle_hook should return None when hook doesn't exist");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_get_hooks() {
        let test_path = format!("/tmp/test-hooks-get-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Initially empty
        let hooks = manager.get_hooks().await;
        assert!(hooks.is_empty(), "Should start with no hooks");

        // Add multiple hooks
        let hook1 = Hook::new("hook1", "Hook 1", TriggerType::FileEdited, Action::run_command("cmd1"));
        let hook2 = Hook::new("hook2", "Hook 2", TriggerType::FileCreated, Action::run_command("cmd2"));
        manager.add_hook(hook1).await.expect("Failed to add hook1");
        manager.add_hook(hook2).await.expect("Failed to add hook2");

        // Get all hooks
        let hooks = manager.get_hooks().await;
        assert_eq!(hooks.len(), 2, "Should have 2 hooks");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_get_hook() {
        let test_path = format!("/tmp/test-hooks-get-one-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add a hook
        let hook = Hook::new(
            "specific-hook",
            "Specific Hook",
            TriggerType::FileEdited,
            Action::run_command("echo test"),
        ).with_priority(50);
        manager.add_hook(hook).await.expect("Failed to add hook");

        // Get the hook by ID
        let found = manager.get_hook("specific-hook").await;
        assert!(found.is_some(), "Should find the hook");
        let found = found.unwrap();
        assert_eq!(found.id, "specific-hook");
        assert_eq!(found.name, "Specific Hook");
        assert_eq!(found.priority, 50);

        // Try to get non-existent hook
        let not_found = manager.get_hook("non-existent").await;
        assert!(not_found.is_none(), "Should not find non-existent hook");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_process_trigger_matching() {
        let test_path = format!("/tmp/test-hooks-process-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add hooks with different triggers
        let hook1 = Hook::new(
            "file-edited-hook",
            "File Edited Hook",
            TriggerType::FileEdited,
            Action::ask_agent("File was edited: {file}"),
        ).with_filter(Filter::file_pattern("*.ts"));
        let hook2 = Hook::new(
            "tool-result-hook",
            "Tool Result Hook",
            TriggerType::ToolResult,
            Action::ask_agent("Tool was called"),
        ).with_filter(Filter::tool_name("Bash"));

        manager.add_hook(hook1).await.expect("Failed to add hook1");
        manager.add_hook(hook2).await.expect("Failed to add hook2");

        // Trigger with file_edited - should match hook1
        let ctx = TriggerContext::file_edited("src/main.ts", "/project");
        let result = manager.process(TriggerType::FileEdited, ctx).await;
        assert_eq!(result.triggered_count, 1, "Should trigger 1 hook");
        assert_eq!(result.pending_actions.len(), 1, "Should have 1 pending action");
        assert!(result.pending_actions[0].prompt.contains("src/main.ts"));

        // Trigger with file_edited for non-matching file - should not trigger
        let ctx = TriggerContext::file_edited("src/main.rs", "/project");
        let result = manager.process(TriggerType::FileEdited, ctx).await;
        assert_eq!(result.triggered_count, 0, "Should not trigger for .rs file");

        // Trigger with tool_result for Bash - should match hook2
        let ctx = TriggerContext::tool_result("Bash", "git status", "output");
        let result = manager.process(TriggerType::ToolResult, ctx).await;
        assert_eq!(result.triggered_count, 1, "Should trigger 1 hook for Bash");

        // Trigger with tool_result for other tool - should not match
        let ctx = TriggerContext::tool_result("FileRead", "file.txt", "content");
        let result = manager.process(TriggerType::ToolResult, ctx).await;
        assert_eq!(result.triggered_count, 0, "Should not trigger for FileRead");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_process_disabled_hook() {
        let test_path = format!("/tmp/test-hooks-disabled-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add a disabled hook
        let hook = Hook::new(
            "disabled-hook",
            "Disabled Hook",
            TriggerType::FileEdited,
            Action::ask_agent("This should not trigger"),
        ).with_enabled(false);
        manager.add_hook(hook).await.expect("Failed to add hook");

        // Trigger - should not match because hook is disabled
        let ctx = TriggerContext::file_edited("src/main.ts", "/project");
        let result = manager.process(TriggerType::FileEdited, ctx).await;
        assert_eq!(result.triggered_count, 0, "Disabled hook should not trigger");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_process_priority_order() {
        let test_path = format!("/tmp/test-hooks-priority-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add hooks with different priorities
        let hook1 = Hook::new(
            "low-priority",
            "Low Priority",
            TriggerType::FileEdited,
            Action::ask_agent("Low"),
        ).with_priority(10);
        let hook2 = Hook::new(
            "high-priority",
            "High Priority",
            TriggerType::FileEdited,
            Action::ask_agent("High"),
        ).with_priority(100);
        let hook3 = Hook::new(
            "medium-priority",
            "Medium Priority",
            TriggerType::FileEdited,
            Action::ask_agent("Medium"),
        ).with_priority(50);

        manager.add_hook(hook1).await.expect("Failed to add hook1");
        manager.add_hook(hook2).await.expect("Failed to add hook2");
        manager.add_hook(hook3).await.expect("Failed to add hook3");

        // Trigger - all should match, executed in priority order
        let ctx = TriggerContext::file_edited("test.ts", "/project");
        let result = manager.process(TriggerType::FileEdited, ctx).await;
        assert_eq!(result.triggered_count, 3, "All 3 hooks should trigger");

        // Verify execution stats were recorded
        let stats = manager.get_stats().await;
        assert_eq!(stats.total_executions, 3);
        assert_eq!(stats.successful_executions, 3);

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_matches_filter_regex() {
        // Test regex filter matching
        let hook = Hook::new(
            "regex-hook",
            "Regex Hook",
            TriggerType::ToolResult,
            Action::run_command("echo matched"),
        ).with_filter(Filter::tool_name("Bash").with_regex("git commit"));

        // Should match - tool is Bash and input contains "git commit"
        let ctx = TriggerContext::tool_result("Bash", "git commit -m 'test'", "output");
        assert!(hook.matches(&TriggerType::ToolResult, &ctx));

        // Should not match - tool is Bash but input doesn't contain "git commit"
        let ctx = TriggerContext::tool_result("Bash", "git status", "output");
        assert!(!hook.matches(&TriggerType::ToolResult, &ctx));

        // Should not match - wrong tool
        let ctx = TriggerContext::tool_result("FileRead", "git commit", "output");
        assert!(!hook.matches(&TriggerType::ToolResult, &ctx));
    }

    #[tokio::test]
    async fn test_update_hook() {
        let test_path = format!("/tmp/test-hooks-update-{}.json", uuid::Uuid::new_v4());
        let manager = HookManager::with_config_path(test_path.clone());

        // Add a hook
        let hook = Hook::new(
            "update-hook",
            "Original Name",
            TriggerType::FileEdited,
            Action::run_command("original"),
        );
        manager.add_hook(hook).await.expect("Failed to add hook");

        // Update the hook
        let updated_hook = Hook::new(
            "update-hook",
            "Updated Name",
            TriggerType::FileCreated,
            Action::run_command("updated"),
        ).with_priority(200);
        let result = manager.update_hook(updated_hook).await;
        assert!(result.is_ok(), "update_hook should succeed");

        // Verify the update
        let hook = manager.get_hook("update-hook").await.expect("Hook should exist");
        assert_eq!(hook.name, "Updated Name");
        assert_eq!(hook.trigger, TriggerType::FileCreated);
        assert_eq!(hook.priority, 200);

        // Try to update non-existent hook
        let non_existent = Hook::new("non-existent", "Name", TriggerType::FileEdited, Action::run_command("cmd"));
        let result = manager.update_hook(non_existent).await;
        assert!(result.is_err(), "update_hook should fail for non-existent hook");

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }
}
