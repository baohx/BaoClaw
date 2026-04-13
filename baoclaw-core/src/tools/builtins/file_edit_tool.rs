use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::tools::trait_def::*;

use super::backup::backup_file_before_write;
use super::path_utils::resolve_and_validate_path;

/// FileEditTool - finds and replaces a unique string in a file
pub struct FileEditTool {
    additional_dirs: Vec<PathBuf>,
}

impl FileEditTool {
    pub fn new(additional_dirs: Vec<PathBuf>) -> Self {
        Self { additional_dirs }
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "FileEdit"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Edit"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "file_path": {
                    "type": "string",
                    "description": "The path to the file to edit"
                },
                "old_string": {
                    "type": "string",
                    "description": "The string to find and replace (must appear exactly once)"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement string"
                }
            })),
            required: Some(vec![
                "file_path".to_string(),
                "old_string".to_string(),
                "new_string".to_string(),
            ]),
            description: Some("Find and replace a unique string in a file".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn prompt(&self) -> String {
        "Edit a file by finding and replacing a specific string. The old_string must appear exactly once in the file.".to_string()
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
        let has_old = input.get("old_string").and_then(|v| v.as_str()).is_some();
        let has_new = input.get("new_string").and_then(|v| v.as_str()).is_some();

        if !has_path {
            return ValidationResult::Invalid {
                message: "Missing or empty 'file_path' field".to_string(),
                code: None,
            };
        }
        if !has_old {
            return ValidationResult::Invalid {
                message: "Missing 'old_string' field".to_string(),
                code: None,
            };
        }
        if !has_new {
            return ValidationResult::Invalid {
                message: "Missing 'new_string' field".to_string(),
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

        let old_string = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'old_string' field".to_string()))?;

        let new_string = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'new_string' field".to_string()))?;

        let resolved = resolve_and_validate_path(file_path_str, &context.cwd, &self.additional_dirs)
            .map_err(|e| ToolError::ExecutionFailed(e))?;

        // Backup existing file before editing
        if let Err(e) = backup_file_before_write(&resolved, &context.cwd).await {
            eprintln!("Warning: failed to backup file: {}", e);
        }

        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

        // Count occurrences
        let count = content.matches(old_string).count();

        if count == 0 {
            return Ok(ToolResult {
                data: json!({
                    "error": "old_string not found in file",
                    "file_path": resolved.to_string_lossy(),
                }),
                is_error: true,
            });
        }

        if count > 1 {
            return Ok(ToolResult {
                data: json!({
                    "error": format!("old_string found {} times, expected exactly 1", count),
                    "file_path": resolved.to_string_lossy(),
                }),
                is_error: true,
            });
        }

        let new_content = content.replacen(old_string, new_string, 1);

        tokio::fs::write(&resolved, &new_content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {}", e)))?;

        Ok(ToolResult {
            data: json!({
                "file_path": resolved.to_string_lossy(),
                "old_string": old_string,
                "new_string": new_string,
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
    async fn test_edit_replaces_unique_string() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let tool = FileEditTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({
                    "file_path": file_path.to_str().unwrap(),
                    "old_string": "hello",
                    "new_string": "goodbye"
                }),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "goodbye world");
    }

    #[tokio::test]
    async fn test_edit_fails_when_string_not_found() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let tool = FileEditTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({
                    "file_path": file_path.to_str().unwrap(),
                    "old_string": "nonexistent",
                    "new_string": "replacement"
                }),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.data["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_edit_fails_when_string_appears_multiple_times() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "aaa bbb aaa").unwrap();

        let tool = FileEditTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({
                    "file_path": file_path.to_str().unwrap(),
                    "old_string": "aaa",
                    "new_string": "ccc"
                }),
                &ctx,
                &progress,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(result.data["error"].as_str().unwrap().contains("2 times"));
        // File should be unchanged
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "aaa bbb aaa");
    }

    #[tokio::test]
    async fn test_edit_path_traversal_rejected() {
        let dir = TempDir::new().unwrap();
        let tool = FileEditTool::new(vec![]);
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(
                json!({
                    "file_path": "../../etc/passwd",
                    "old_string": "root",
                    "new_string": "evil"
                }),
                &ctx,
                &progress,
            )
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_file_edit_properties() {
        let tool = FileEditTool::new(vec![]);
        assert_eq!(tool.name(), "FileEdit");
        assert!(!tool.is_read_only(&json!({})));
        assert!(!tool.is_concurrency_safe(&json!({})));
    }
}
