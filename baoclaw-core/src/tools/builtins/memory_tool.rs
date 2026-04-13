use async_trait::async_trait;
use serde_json::{json, Value};
use std::io::Write;

use crate::tools::trait_def::*;

/// Tool that allows the AI to automatically save important information
/// to long-term memory (user preferences, facts, decisions).
pub struct MemoryTool;

impl MemoryTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str { "MemoryTool" }

    fn aliases(&self) -> Vec<&str> { vec!["Memory", "SaveMemory"] }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "content": { "type": "string", "description": "The information to remember" },
                "category": {
                    "type": "string",
                    "enum": ["fact", "preference", "decision"],
                    "description": "Category: fact (user told me X), preference (user prefers Y), decision (we decided Z)"
                }
            })),
            required: Some(vec!["content".to_string(), "category".to_string()]),
            description: Some("Save important information to long-term memory that persists across sessions".to_string()),
        }
    }

    fn prompt(&self) -> String {
        "Save important information to long-term memory. Use this when you discover user preferences, \
         important facts, or decisions that should be remembered across conversations. \
         Categories: 'fact' for things the user told you, 'preference' for user preferences and habits, \
         'decision' for decisions made during conversations. Be concise — store the key information only."
            .to_string()
    }

    async fn call(
        &self,
        input: Value,
        _context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let content = input.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'content'".into()))?;
        let category = input.get("category").and_then(|v| v.as_str()).unwrap_or("fact");

        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let memory_path = std::path::PathBuf::from(&home).join(".baoclaw").join("memory.jsonl");

        let entry = json!({
            "id": &uuid::Uuid::new_v4().to_string()[..8],
            "content": content,
            "category": category,
            "created_at": chrono::Utc::now().to_rfc3339(),
            "source": "auto"
        });

        let line = serde_json::to_string(&entry)
            .map_err(|e| ToolError::ExecutionFailed(format!("Serialize error: {}", e)))?;

        let mut f = std::fs::OpenOptions::new()
            .create(true).append(true).open(&memory_path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to open memory file: {}", e)))?;
        writeln!(f, "{}", line)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write: {}", e)))?;

        Ok(ToolResult {
            data: json!({ "saved": true, "id": entry["id"], "content": content, "category": category }),
            is_error: false,
        })
    }
}
