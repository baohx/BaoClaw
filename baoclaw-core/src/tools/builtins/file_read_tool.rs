use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::tools::trait_def::*;

use super::path_utils::resolve_and_validate_path;

/// FileReadTool - reads file content with optional line range
pub struct FileReadTool {
    additional_dirs: Vec<PathBuf>,
}

impl FileReadTool {
    pub fn new(additional_dirs: Vec<PathBuf>) -> Self {
        Self { additional_dirs }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "FileRead"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Read"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "file_path": {
                    "type": "string",
                    "description": "The path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line offset to start reading from (0-based)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                }
            })),
            required: Some(vec!["file_path".to_string()]),
            description: Some("Read file content with optional line range".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn prompt(&self) -> String {
        "Read the contents of a file. Supports reading specific line ranges with offset and limit parameters.".to_string()
    }

    async fn validate_input(
        &self,
        input: &Value,
        _context: &ToolContext,
    ) -> ValidationResult {
        match input.get("file_path").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => ValidationResult::Ok,
            _ => ValidationResult::Invalid {
                message: "Missing or empty 'file_path' field".to_string(),
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
        let file_path_str = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'file_path' field".to_string()))?;

        let resolved = resolve_and_validate_path(file_path_str, &context.cwd, &self.additional_dirs)
            .map_err(|e| ToolError::ExecutionFailed(e))?;

        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64());

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start = offset.min(total_lines);
        let end = match limit {
            Some(l) => (start + l as usize).min(total_lines),
            None => total_lines,
        };

        let selected: Vec<&str> = lines[start..end].to_vec();
        let result_content = selected.join("\n");

        Ok(ToolResult {
            data: json!({
                "content": result_content,
                "file_path": resolved.to_string_lossy(),
                "total_lines": total_lines,
                "lines_read": end - start,
                "offset": start,
            }),
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

    fn make_context(cwd: &std::path::Path) -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: cwd.to_path_buf(),
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
        }
    }

    #[tokio::test]
    async fn test_read_entire_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line1\nline2\nline3").unwrap();

        let tool = FileReadTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({"file_path": file_path.to_str().unwrap()}),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.data["content"], "line1\nline2\nline3");
        assert_eq!(result.data["total_lines"], 3);
    }

    #[tokio::test]
    async fn test_read_with_offset_and_limit() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "a\nb\nc\nd\ne").unwrap();

        let tool = FileReadTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({"file_path": file_path.to_str().unwrap(), "offset": 1, "limit": 2}),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.data["content"], "b\nc");
        assert_eq!(result.data["lines_read"], 2);
        assert_eq!(result.data["offset"], 1);
    }

    #[tokio::test]
    async fn test_read_nonexistent_file() {
        let dir = TempDir::new().unwrap();
        let tool = FileReadTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({"file_path": "nonexistent.txt"}),
                &ctx,
                &progress,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_path_traversal_rejected() {
        let dir = TempDir::new().unwrap();
        let tool = FileReadTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({"file_path": "../../etc/passwd"}),
                &ctx,
                &progress,
            )
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_file_read_properties() {
        let tool = FileReadTool::new(vec![]);
        assert_eq!(tool.name(), "FileRead");
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));
    }
}
