use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// Tool execution result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub data: Value,
    pub is_error: bool,
}

/// Input validation result
#[derive(Clone, Debug)]
pub enum ValidationResult {
    Ok,
    Invalid { message: String, code: Option<String> },
}

/// Permission check result from a tool's perspective
#[derive(Clone, Debug)]
pub enum ToolPermissionCheckResult {
    Allow { updated_input: Value },
    Ask { message: String, updated_input: Value },
    Deny { message: String },
}

/// Progress sender trait for tools to report progress
#[async_trait]
pub trait ProgressSender: Send + Sync {
    async fn send_progress(&self, tool_use_id: &str, data: Value);
}

/// JSON Schema representation for tool input
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The core Tool trait that all tools must implement
#[async_trait]
pub trait Tool: Send + Sync {
    /// The unique name of this tool
    fn name(&self) -> &str;

    /// Alternative names for this tool
    fn aliases(&self) -> Vec<&str> {
        vec![]
    }

    /// JSON Schema for the tool's input
    fn input_schema(&self) -> JsonSchema;

    /// Whether this tool only reads data (doesn't modify filesystem)
    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    /// Whether this tool is destructive (e.g., deletes files)
    fn is_destructive(&self, _input: &Value) -> bool {
        false
    }

    /// Whether this tool can be safely executed concurrently with other tools
    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    /// Whether this tool is currently enabled
    fn is_enabled(&self) -> bool {
        true
    }

    /// Maximum result size in characters before persisting to disk
    fn max_result_size_chars(&self) -> usize {
        100_000
    }

    /// Execute the tool with the given input
    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError>;

    /// Validate the input before execution
    async fn validate_input(
        &self,
        _input: &Value,
        _context: &ToolContext,
    ) -> ValidationResult {
        ValidationResult::Ok
    }

    /// Tool-specific permission check
    async fn check_permissions(
        &self,
        _input: &Value,
        _context: &ToolContext,
    ) -> ToolPermissionCheckResult {
        ToolPermissionCheckResult::Ask {
            message: format!("Tool '{}' requires permission", self.name()),
            updated_input: Value::Null,
        }
    }

    /// Get the system prompt contribution for this tool
    fn prompt(&self) -> String;

    /// User-facing display name
    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        self.name().to_string()
    }
}

/// Context available to tools during execution
#[derive(Clone)]
pub struct ToolContext {
    pub cwd: PathBuf,
    pub model: String,
    pub abort_signal: Arc<tokio::sync::watch::Receiver<bool>>,
}

/// Tool execution errors
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Tool timed out after {0}ms")]
    Timeout(u64),
    #[error("Tool was aborted")]
    Aborted,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
