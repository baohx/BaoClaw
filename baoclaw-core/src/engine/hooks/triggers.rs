//! Trigger types for the hook system.
//!
//! This module defines the types of events that can trigger hooks and their
//! associated context data.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Types of events that can trigger hooks.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    /// File was modified after a write operation
    FileEdited,
    /// File was created
    FileCreated,
    /// File was deleted
    FileDeleted,
    /// Tool execution completed
    ToolResult,
    /// Session started
    SessionStart,
    /// Session ended
    SessionEnd,
    /// User sent a message
    UserMessage,
    /// Assistant sent a message
    AssistantMessage,
    /// An error occurred
    Error,
}

impl std::fmt::Display for TriggerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileEdited => write!(f, "file_edited"),
            Self::FileCreated => write!(f, "file_created"),
            Self::FileDeleted => write!(f, "file_deleted"),
            Self::ToolResult => write!(f, "tool_result"),
            Self::SessionStart => write!(f, "session_start"),
            Self::SessionEnd => write!(f, "session_end"),
            Self::UserMessage => write!(f, "user_message"),
            Self::AssistantMessage => write!(f, "assistant_message"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for TriggerType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "file_edited" => Ok(Self::FileEdited),
            "file_created" => Ok(Self::FileCreated),
            "file_deleted" => Ok(Self::FileDeleted),
            "tool_result" => Ok(Self::ToolResult),
            "session_start" => Ok(Self::SessionStart),
            "session_end" => Ok(Self::SessionEnd),
            "user_message" => Ok(Self::UserMessage),
            "assistant_message" => Ok(Self::AssistantMessage),
            "error" => Ok(Self::Error),
            _ => Err(format!("Unknown trigger type: {}", s)),
        }
    }
}

/// Filter conditions for hook matching.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Filter {
    /// Glob pattern for file-based triggers (e.g., "*.ts", "src/**/*.py")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_pattern: Option<String>,

    /// Tool name for tool_result trigger
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    /// Error type filter for error trigger
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,

    /// Regex pattern to match against relevant data
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
}

impl Filter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a filter with a file pattern.
    pub fn file_pattern(pattern: impl Into<String>) -> Self {
        Self {
            file_pattern: Some(pattern.into()),
            ..Default::default()
        }
    }

    /// Create a filter with a tool name.
    pub fn tool_name(name: impl Into<String>) -> Self {
        Self {
            tool_name: Some(name.into()),
            ..Default::default()
        }
    }

    /// Add a regex pattern.
    pub fn with_regex(mut self, pattern: impl Into<String>) -> Self {
        self.regex = Some(pattern.into());
        self
    }

    /// Check if a file path matches this filter.
    pub fn matches_file(&self, path: &std::path::Path) -> bool {
        if let Some(pattern) = &self.file_pattern {
            // Use glob pattern matching
            if let Ok(glob) = glob::Pattern::new(pattern) {
                if let Some(path_str) = path.to_str() {
                    return glob.matches(path_str);
                }
            }
            // Fallback: simple extension check for patterns like "*.ts"
            if pattern.starts_with("*.") {
                let ext = &pattern[1..]; // Get ".ts"
                return path.extension().map(|e| e == &ext[1..]).unwrap_or(false);
            }
        }
        true // No file filter means match all
    }

    /// Check if a tool name matches this filter.
    pub fn matches_tool(&self, tool: &str) -> bool {
        if let Some(name) = &self.tool_name {
            return name == tool;
        }
        true
    }

    /// Check if a string matches the regex pattern.
    pub fn matches_regex(&self, text: &str) -> bool {
        if let Some(pattern) = &self.regex {
            if let Ok(re) = Regex::new(pattern) {
                return re.is_match(text);
            }
        }
        true
    }
}

/// Context data provided to hooks when triggered.
#[derive(Clone, Debug, Default)]
pub struct TriggerContext {
    /// The working directory
    pub cwd: Option<PathBuf>,

    /// File path for file-based triggers
    pub file: Option<PathBuf>,

    /// Tool name for tool_result trigger
    pub tool: Option<String>,

    /// Tool input for tool_result trigger
    pub tool_input: Option<String>,

    /// Tool output for tool_result trigger
    pub tool_output: Option<String>,

    /// Error message for error trigger
    pub error: Option<String>,

    /// Session ID for session triggers
    pub session_id: Option<String>,

    /// User message for user_message trigger
    pub user_message: Option<String>,

    /// Assistant message for assistant_message trigger
    pub assistant_message: Option<String>,

    /// Number of turns for session_end trigger
    pub turns: Option<u32>,

    /// Cost for session_end trigger
    pub cost: Option<f64>,
}

impl TriggerContext {
    /// Create a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create context for file-edited event.
    pub fn file_edited(path: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            file: Some(path.into()),
            cwd: Some(cwd.into()),
            ..Default::default()
        }
    }

    /// Create context for file-created event.
    pub fn file_created(path: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            file: Some(path.into()),
            cwd: Some(cwd.into()),
            ..Default::default()
        }
    }

    /// Create context for file-deleted event.
    pub fn file_deleted(path: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            file: Some(path.into()),
            cwd: Some(cwd.into()),
            ..Default::default()
        }
    }

    /// Create context for tool-result event.
    pub fn tool_result(tool: impl Into<String>, input: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            tool: Some(tool.into()),
            tool_input: Some(input.into()),
            tool_output: Some(output.into()),
            ..Default::default()
        }
    }

    /// Create context for session-start event.
    pub fn session_start(session_id: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            cwd: Some(cwd.into()),
            ..Default::default()
        }
    }

    /// Create context for session-end event.
    pub fn session_end(session_id: impl Into<String>, turns: u32, cost: f64) -> Self {
        Self {
            session_id: Some(session_id.into()),
            turns: Some(turns),
            cost: Some(cost),
            ..Default::default()
        }
    }

    /// Create context for user-message event.
    pub fn user_message(message: impl Into<String>) -> Self {
        Self {
            user_message: Some(message.into()),
            ..Default::default()
        }
    }

    /// Create context for assistant-message event.
    pub fn assistant_message(message: impl Into<String>) -> Self {
        Self {
            assistant_message: Some(message.into()),
            ..Default::default()
        }
    }

    /// Create context for error event.
    pub fn error(error: impl Into<String>, tool: Option<String>) -> Self {
        Self {
            error: Some(error.into()),
            tool,
            ..Default::default()
        }
    }

    /// Add a working directory to the context.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Add a session ID to the context.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Get a variable value by name for template substitution.
    pub fn get_variable(&self, name: &str) -> Option<String> {
        match name {
            "file" => self.file.as_ref().and_then(|p| p.to_str()).map(|s| s.to_string()),
            "cwd" => self.cwd.as_ref().and_then(|p| p.to_str()).map(|s| s.to_string()),
            "tool" => self.tool.clone(),
            "input" => self.tool_input.clone(),
            "output" => self.tool_output.clone(),
            "error" => self.error.clone(),
            "session_id" => self.session_id.clone(),
            "user_message" => self.user_message.clone(),
            "assistant_message" => self.assistant_message.clone(),
            "turns" => self.turns.map(|t| t.to_string()),
            "cost" => self.cost.map(|c| c.to_string()),
            _ => None,
        }
    }
}

/// A trigger definition with its filter conditions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trigger {
    /// The type of event that triggers this hook
    #[serde(rename = "type")]
    pub trigger_type: TriggerType,

    /// Optional filter conditions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
}

impl Trigger {
    /// Create a new trigger of the given type.
    pub fn new(trigger_type: TriggerType) -> Self {
        Self {
            trigger_type,
            filter: None,
        }
    }

    /// Add a filter to this trigger.
    pub fn with_filter(mut self, filter: Filter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Check if this trigger matches the given context.
    pub fn matches(&self, trigger_type: &TriggerType, ctx: &TriggerContext) -> bool {
        if &self.trigger_type != trigger_type {
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

            // Check regex filter against relevant data
            let regex_text = match &self.trigger_type {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_trigger_type_from_str() {
        assert_eq!(
            TriggerType::from_str("file_edited").unwrap(),
            TriggerType::FileEdited
        );
        assert_eq!(
            TriggerType::from_str("tool_result").unwrap(),
            TriggerType::ToolResult
        );
        assert!(TriggerType::from_str("invalid").is_err());
    }

    #[test]
    fn test_trigger_type_display() {
        assert_eq!(TriggerType::FileEdited.to_string(), "file_edited");
        assert_eq!(TriggerType::ToolResult.to_string(), "tool_result");
    }

    #[test]
    fn test_filter_matches_file() {
        let filter = Filter::file_pattern("*.ts");
        assert!(filter.matches_file(PathBuf::from("src/main.ts").as_path()));
        assert!(!filter.matches_file(PathBuf::from("src/main.rs").as_path()));
    }

    #[test]
    fn test_filter_matches_tool() {
        let filter = Filter::tool_name("Bash");
        assert!(filter.matches_tool("Bash"));
        assert!(!filter.matches_tool("FileRead"));
    }

    #[test]
    fn test_trigger_matches() {
        let trigger = Trigger::new(TriggerType::FileEdited)
            .with_filter(Filter::file_pattern("*.ts"));

        let ctx = TriggerContext::file_edited("src/main.ts", "/project");
        assert!(trigger.matches(&TriggerType::FileEdited, &ctx));

        let ctx = TriggerContext::file_edited("src/main.rs", "/project");
        assert!(!trigger.matches(&TriggerType::FileEdited, &ctx));
    }

    #[test]
    fn test_context_get_variable() {
        let ctx = TriggerContext::file_edited("src/main.ts", "/project");
        assert_eq!(ctx.get_variable("file"), Some("src/main.ts".to_string()));
        assert_eq!(ctx.get_variable("cwd"), Some("/project".to_string()));
        assert_eq!(ctx.get_variable("error"), None);
    }
}
