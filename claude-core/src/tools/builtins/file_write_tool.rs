use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::tools::trait_def::*;

use super::path_utils::resolve_and_validate_path;

/// FileWriteTool - writes content to a file, creating parent dirs if needed
pub struct FileWriteTool {
    additional_dirs: Vec<PathBuf>,
}

impl FileWriteTool {
    pub fn new(additional_dirs: Vec<PathBuf>) -> Self {
        Self { additional_dirs }
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "FileWrite"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Write"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "file_path": {
                    "type": "string",
                    "description": "The path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            })),
            required: Some(vec!["file_path".to_string(), "content".to_string()]),
            description: Some("Write content to a file".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn prompt(&self) -> String {
        "Write content to a file. Creates parent directories if they don't exist.".to_string()
    }

    async fn validate_input(
        &self,
        input: &Value,
        _context: &ToolContext,
    ) -> ValidationResult {
        let has_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map_or(false, |s| !s.is_empty());
        let has_content = input.get("content").and_then(|v| v.as_str()).is_some();

        if !has_path {
            return ValidationResult::Invalid {
                message: "Missing or empty 'file_path' field".to_string(),
                code: None,
            };
        }
        if !has_content {
            return ValidationResult::Invalid {
                message: "Missing 'content' field".to_string(),
                code: None,
            };
        }
        ValidationResult::Ok
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

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'content' field".to_string()))?;

        let resolved = resolve_and_validate_path(file_path_str, &context.cwd, &self.additional_dirs)
            .map_err(|e| ToolError::ExecutionFailed(e))?;

        // Create parent directories if needed
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create directories: {}", e)))?;
        }

        tokio::fs::write(&resolved, content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {}", e)))?;

        Ok(ToolResult {
            data: json!({
                "file_path": resolved.to_string_lossy(),
                "bytes_written": content.len(),
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
    async fn test_write_new_file() {
        let dir = TempDir::new().unwrap();
        let tool = FileWriteTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let file_path = dir.path().join("output.txt");
        let result = tool
            .call(
                json!({
                    "file_path": file_path.to_str().unwrap(),
                    "content": "hello world"
                }),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello world");
    }

    #[tokio::test]
    async fn test_write_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let tool = FileWriteTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let file_path = dir.path().join("a/b/c/deep.txt");
        let result = tool
            .call(
                json!({
                    "file_path": file_path.to_str().unwrap(),
                    "content": "deep content"
                }),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "deep content");
    }

    #[tokio::test]
    async fn test_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("existing.txt");
        std::fs::write(&file_path, "old content").unwrap();

        let tool = FileWriteTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({
                    "file_path": file_path.to_str().unwrap(),
                    "content": "new content"
                }),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "new content");
    }

    #[tokio::test]
    async fn test_write_path_traversal_rejected() {
        let dir = TempDir::new().unwrap();
        let tool = FileWriteTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({
                    "file_path": "../../etc/evil",
                    "content": "bad"
                }),
                &ctx,
                &progress,
            )
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_file_write_properties() {
        let tool = FileWriteTool::new(vec![]);
        assert_eq!(tool.name(), "FileWrite");
        assert!(!tool.is_read_only(&json!({})));
        assert!(!tool.is_concurrency_safe(&json!({})));
    }
}
