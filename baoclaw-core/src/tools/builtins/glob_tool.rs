use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::trait_def::*;

/// Maximum number of glob results before truncation
const MAX_GLOB_RESULTS: usize = 1000;

/// GlobTool — searches for files by name pattern using glob syntax.
/// Returns matching file paths relative to the current working directory.
pub struct GlobTool;

impl GlobTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "GlobTool"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Glob", "FindFiles"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g. **/*.rs, src/**/*.ts)"
                },
                "path": {
                    "type": "string",
                    "description": "Root directory to search (defaults to cwd)"
                }
            })),
            required: Some(vec!["pattern".to_string()]),
            description: Some("Search for files by name pattern using glob syntax".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn prompt(&self) -> String {
        "Search for files by name pattern using glob syntax. \
         Returns matching file paths relative to cwd."
            .to_string()
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        match input.get("pattern").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => ValidationResult::Ok,
            _ => ValidationResult::Invalid {
                message: "Missing or empty 'pattern' field".to_string(),
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
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'pattern' field".to_string()))?;

        let base = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => context.cwd.join(p),
            None => context.cwd.clone(),
        };

        let full_pattern = base.join(pattern).to_string_lossy().to_string();

        let entries = glob::glob(&full_pattern)
            .map_err(|e| ToolError::ExecutionFailed(format!("Invalid glob pattern: {}", e)))?;

        let mut files: Vec<String> = Vec::new();
        let mut truncated = false;

        for entry in entries {
            if let Ok(path) = entry {
                if path.is_file() {
                    let relative = path
                        .strip_prefix(&context.cwd)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();
                    files.push(relative);
                }
                if files.len() >= MAX_GLOB_RESULTS {
                    truncated = true;
                    break;
                }
            }
        }

        Ok(ToolResult {
            data: json!({
                "files": files,
                "count": files.len(),
                "truncated": truncated,
            }),
            is_error: false,
        })
    }
}

/// Core glob search function exposed for testing.
///
/// Searches for files matching the given glob pattern under `base_dir`,
/// returning paths relative to `cwd`. Results are capped at `max_results`.
pub fn glob_search(
    pattern: &str,
    base_dir: &std::path::Path,
    cwd: &std::path::Path,
    max_results: usize,
) -> Result<GlobResult, ToolError> {
    let full_pattern = base_dir.join(pattern).to_string_lossy().to_string();

    let entries = glob::glob(&full_pattern)
        .map_err(|e| ToolError::ExecutionFailed(format!("Invalid glob pattern: {}", e)))?;

    let mut files: Vec<String> = Vec::new();
    let mut truncated = false;

    for entry in entries {
        if let Ok(path) = entry {
            if path.is_file() {
                let relative = path
                    .strip_prefix(cwd)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                files.push(relative);
            }
            if files.len() >= max_results {
                truncated = true;
                break;
            }
        }
    }

    Ok(GlobResult {
        files,
        truncated,
    })
}

/// Result of a glob search operation.
#[derive(Debug)]
pub struct GlobResult {
    pub files: Vec<String>,
    pub truncated: bool,
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

    #[test]
    fn test_glob_tool_properties() {
        let tool = GlobTool::new();
        assert_eq!(tool.name(), "GlobTool");
        assert_eq!(tool.aliases(), vec!["Glob", "FindFiles"]);
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_glob_search_valid_pattern() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("hello.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("world.rs"), "fn test() {}").unwrap();
        std::fs::write(dir.path().join("readme.md"), "# Readme").unwrap();

        let result = glob_search("*.rs", dir.path(), dir.path(), 100).unwrap();
        assert_eq!(result.files.len(), 2);
        assert!(!result.truncated);
        for f in &result.files {
            assert!(f.ends_with(".rs"), "Expected .rs file, got: {}", f);
        }
    }

    #[test]
    fn test_glob_search_invalid_pattern() {
        let dir = TempDir::new().unwrap();
        let result = glob_search("[invalid", dir.path(), dir.path(), 100);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("Invalid glob"));
    }

    #[test]
    fn test_glob_search_relative_paths() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("lib.rs"), "// lib").unwrap();

        let result = glob_search("src/*.rs", dir.path(), dir.path(), 100).unwrap();
        assert_eq!(result.files.len(), 1);
        // Path should be relative (not absolute)
        let path = &result.files[0];
        assert!(!std::path::Path::new(path).is_absolute(), "Path should be relative: {}", path);
        assert!(path.contains("src"), "Path should contain 'src': {}", path);
    }

    #[test]
    fn test_glob_search_truncation() {
        let dir = TempDir::new().unwrap();
        // Create more files than the limit
        for i in 0..15 {
            std::fs::write(dir.path().join(format!("file_{}.txt", i)), "content").unwrap();
        }

        let result = glob_search("*.txt", dir.path(), dir.path(), 5).unwrap();
        assert_eq!(result.files.len(), 5);
        assert!(result.truncated);
    }

    #[test]
    fn test_glob_search_no_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.rs"), "fn main() {}").unwrap();

        let result = glob_search("*.py", dir.path(), dir.path(), 100).unwrap();
        assert!(result.files.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn test_glob_search_recursive_pattern() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.path().join("top.rs"), "// top").unwrap();
        std::fs::write(sub.join("deep.rs"), "// deep").unwrap();

        let result = glob_search("**/*.rs", dir.path(), dir.path(), 100).unwrap();
        assert_eq!(result.files.len(), 2);
    }

    #[tokio::test]
    async fn test_glob_tool_call_valid() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("lib.rs"), "// lib").unwrap();

        let tool = GlobTool::new();
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"pattern": "*.rs"}), &ctx, &progress)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.data["count"], 2);
        assert!(!result.data["truncated"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_glob_tool_call_invalid_pattern() {
        let dir = TempDir::new().unwrap();
        let tool = GlobTool::new();
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"pattern": "[invalid"}), &ctx, &progress)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_glob_tool_validate_missing_pattern() {
        let tool = GlobTool::new();
        let dir = TempDir::new().unwrap();
        let ctx = make_context(dir.path());

        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_glob_tool_validate_empty_pattern() {
        let tool = GlobTool::new();
        let dir = TempDir::new().unwrap();
        let ctx = make_context(dir.path());

        let result = tool.validate_input(&json!({"pattern": ""}), &ctx).await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_glob_tool_with_path_param() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("test.rs"), "// test").unwrap();
        std::fs::write(dir.path().join("root.rs"), "// root").unwrap();

        let tool = GlobTool::new();
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"pattern": "*.rs", "path": "subdir"}), &ctx, &progress)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.data["count"], 1);
    }
}
