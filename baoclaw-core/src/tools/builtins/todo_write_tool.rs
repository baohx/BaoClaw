use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tools::trait_def::*;

/// TODO list management tool — stores items in .baoclaw/todo.json
pub struct TodoWriteTool;

impl TodoWriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoItem {
    text: String,
    completed: bool,
    priority: String,
    created_at: String,
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWriteTool"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Todo"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "operation": {
                    "type": "string",
                    "enum": ["add", "complete", "remove", "list"]
                },
                "text": { "type": "string", "description": "TODO content (for add)" },
                "index": { "type": "integer", "description": "TODO index (for complete/remove)" },
                "priority": { "type": "string", "enum": ["high", "medium", "low"] }
            })),
            required: Some(vec!["operation".to_string()]),
            description: Some("Manage project TODO list".to_string()),
        }
    }

    fn prompt(&self) -> String {
        "Manage a project TODO list. Supports add, complete, remove, and list operations."
            .to_string()
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'operation'".to_string()))?;

        let todo_dir = context.cwd.join(".baoclaw");
        let todo_path = todo_dir.join("todo.json");

        // Load existing items
        let mut items: Vec<TodoItem> = if todo_path.exists() {
            let content = tokio::fs::read_to_string(&todo_path)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read todo.json: {}", e)))?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        let result = match operation {
            "add" => {
                let text = input
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::ExecutionFailed("Missing 'text' for add operation".to_string())
                    })?;
                let priority = input
                    .get("priority")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium")
                    .to_string();
                let item = TodoItem {
                    text: text.to_string(),
                    completed: false,
                    priority,
                    created_at: chrono::Utc::now().to_rfc3339(),
                };
                items.push(item.clone());
                json!({ "added": item, "total": items.len() })
            }
            "complete" => {
                let index = input
                    .get("index")
                    .and_then(|v| {
                        v.as_u64().or_else(|| {
                            v.as_str().and_then(|s| s.parse::<u64>().ok())
                        })
                    })
                    .map(|v| v as usize)
                    .ok_or_else(|| {
                        ToolError::ExecutionFailed("Missing 'index' for complete operation".to_string())
                    })?;
                if index >= items.len() {
                    return Err(ToolError::ExecutionFailed(format!(
                        "index {} out of range (0..{})",
                        index,
                        items.len()
                    )));
                }
                items[index].completed = true;
                json!({ "completed": items[index], "total": items.len() })
            }
            "remove" => {
                let index = input
                    .get("index")
                    .and_then(|v| {
                        v.as_u64().or_else(|| {
                            v.as_str().and_then(|s| s.parse::<u64>().ok())
                        })
                    })
                    .map(|v| v as usize)
                    .ok_or_else(|| {
                        ToolError::ExecutionFailed("Missing 'index' for remove operation".to_string())
                    })?;
                if index >= items.len() {
                    return Err(ToolError::ExecutionFailed(format!(
                        "index {} out of range (0..{})",
                        index,
                        items.len()
                    )));
                }
                let removed = items.remove(index);
                json!({ "removed": removed, "total": items.len() })
            }
            "list" => {
                json!({ "items": items, "total": items.len() })
            }
            other => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Unknown operation: {}",
                    other
                )));
            }
        };

        // Persist changes (except for list which is read-only)
        if operation != "list" {
            tokio::fs::create_dir_all(&todo_dir)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create .baoclaw dir: {}", e)))?;
            let serialized = serde_json::to_string_pretty(&items)
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to serialize: {}", e)))?;
            tokio::fs::write(&todo_path, serialized)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write todo.json: {}", e)))?;
        }

        Ok(ToolResult {
            data: result,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct NoopProgress;
    #[async_trait]
    impl ProgressSender for NoopProgress {
        async fn send_progress(&self, _id: &str, _data: Value) {}
    }

    fn make_context(cwd: std::path::PathBuf) -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd,
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
        }
    }

    #[test]
    fn test_tool_name() {
        let tool = TodoWriteTool::new();
        assert_eq!(tool.name(), "TodoWriteTool");
    }

    #[test]
    fn test_tool_aliases() {
        let tool = TodoWriteTool::new();
        assert_eq!(tool.aliases(), vec!["Todo"]);
    }

    #[tokio::test]
    async fn test_add_item() {
        let tmp = TempDir::new().unwrap();
        let tool = TodoWriteTool::new();
        let ctx = make_context(tmp.path().to_path_buf());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({"operation": "add", "text": "Write tests", "priority": "high"}),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.data["total"], 1);
        assert_eq!(result.data["added"]["text"], "Write tests");
        assert_eq!(result.data["added"]["priority"], "high");
        assert_eq!(result.data["added"]["completed"], false);
    }

    #[tokio::test]
    async fn test_list_empty() {
        let tmp = TempDir::new().unwrap();
        let tool = TodoWriteTool::new();
        let ctx = make_context(tmp.path().to_path_buf());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"operation": "list"}), &ctx, &progress)
            .await
            .unwrap();

        assert_eq!(result.data["total"], 0);
        assert_eq!(result.data["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_add_then_complete() {
        let tmp = TempDir::new().unwrap();
        let tool = TodoWriteTool::new();
        let ctx = make_context(tmp.path().to_path_buf());
        let progress = NoopProgress;

        tool.call(
            json!({"operation": "add", "text": "Task 1"}),
            &ctx,
            &progress,
        )
        .await
        .unwrap();

        let result = tool
            .call(json!({"operation": "complete", "index": 0}), &ctx, &progress)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.data["completed"]["completed"], true);
    }

    #[tokio::test]
    async fn test_add_then_remove() {
        let tmp = TempDir::new().unwrap();
        let tool = TodoWriteTool::new();
        let ctx = make_context(tmp.path().to_path_buf());
        let progress = NoopProgress;

        tool.call(
            json!({"operation": "add", "text": "Task 1"}),
            &ctx,
            &progress,
        )
        .await
        .unwrap();

        let result = tool
            .call(json!({"operation": "remove", "index": 0}), &ctx, &progress)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.data["total"], 0);
        assert_eq!(result.data["removed"]["text"], "Task 1");
    }

    #[tokio::test]
    async fn test_complete_out_of_bounds() {
        let tmp = TempDir::new().unwrap();
        let tool = TodoWriteTool::new();
        let ctx = make_context(tmp.path().to_path_buf());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"operation": "complete", "index": 0}), &ctx, &progress)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_out_of_bounds() {
        let tmp = TempDir::new().unwrap();
        let tool = TodoWriteTool::new();
        let ctx = make_context(tmp.path().to_path_buf());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"operation": "remove", "index": 5}), &ctx, &progress)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_add_missing_text() {
        let tmp = TempDir::new().unwrap();
        let tool = TodoWriteTool::new();
        let ctx = make_context(tmp.path().to_path_buf());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"operation": "add"}), &ctx, &progress)
            .await;

        assert!(result.is_err());
    }
}
