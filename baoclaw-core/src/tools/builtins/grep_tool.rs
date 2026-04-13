use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

use crate::tools::trait_def::*;

/// Maximum number of grep results before truncation
const MAX_RESULTS: usize = 500;

/// A single grep match result
#[derive(Clone, Debug, Serialize)]
pub struct GrepMatch {
    pub file: String,
    pub line_number: usize,
    pub content: String,
    pub context: Vec<String>,
}

/// GrepTool — searches file contents using regex patterns.
/// Respects .gitignore rules via the `ignore` crate.
pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "GrepTool"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Grep", "Search"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file path to search (defaults to cwd)"
                },
                "include": {
                    "type": "string",
                    "description": "File name glob filter (e.g. *.rs)"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines before and after each match (default 2)"
                }
            })),
            required: Some(vec!["pattern".to_string()]),
            description: Some(
                "Search file contents using regex. Returns matching lines with context.".to_string(),
            ),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn prompt(&self) -> String {
        "Search file contents using regex. Returns matching lines with context. \
         Use this to find code patterns, function definitions, imports, etc."
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

        let search_path = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => context.cwd.join(p),
            None => context.cwd.clone(),
        };

        let include_glob = input.get("include").and_then(|v| v.as_str());
        let context_lines = input
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as usize;

        let matches =
            grep_search(pattern, &search_path, include_glob, context_lines, MAX_RESULTS)?;

        let truncated = matches.len() >= MAX_RESULTS;

        Ok(ToolResult {
            data: json!({
                "matches": matches,
                "count": matches.len(),
                "truncated": truncated,
            }),
            is_error: false,
        })
    }
}

/// Core grep search algorithm.
///
/// Uses the `ignore` crate to walk files (respecting .gitignore) and the
/// `regex` crate to match lines. Returns up to `max_results` matches.
pub fn grep_search(
    pattern: &str,
    search_path: &Path,
    include_glob: Option<&str>,
    context_lines: usize,
    max_results: usize,
) -> Result<Vec<GrepMatch>, ToolError> {
    let regex = Regex::new(pattern)
        .map_err(|e| ToolError::ExecutionFailed(format!("Invalid regex: {}", e)))?;

    let mut matches = Vec::new();

    let mut builder = WalkBuilder::new(search_path);
    builder.hidden(false).git_ignore(true).git_global(true);

    // Apply include glob filter if provided
    if let Some(glob_pattern) = include_glob {
        let mut types_builder = ignore::types::TypesBuilder::new();
        types_builder.add("custom", glob_pattern).map_err(|e| {
            ToolError::ExecutionFailed(format!("Invalid include glob '{}': {}", glob_pattern, e))
        })?;
        types_builder.select("custom");
        builder.types(types_builder.build().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to build type matcher: {}", e))
        })?);
    }

    let walker = builder.build();

    for entry in walker {
        if matches.len() >= max_results {
            break;
        }

        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Read file content, skip binary/unreadable files
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        for (line_idx, line) in lines.iter().enumerate() {
            if regex.is_match(line) {
                let start = line_idx.saturating_sub(context_lines);
                let end = (line_idx + context_lines + 1).min(lines.len());
                let context: Vec<String> = lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{}: {}", start + i + 1, l))
                    .collect();

                matches.push(GrepMatch {
                    file: path.to_string_lossy().to_string(),
                    line_number: line_idx + 1,
                    content: line.to_string(),
                    context,
                });

                if matches.len() >= max_results {
                    break;
                }
            }
        }
    }

    Ok(matches)
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
    fn test_grep_tool_properties() {
        let tool = GrepTool::new();
        assert_eq!(tool.name(), "GrepTool");
        assert_eq!(tool.aliases(), vec!["Grep", "Search"]);
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_grep_search_valid_regex() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

        let results = grep_search("fn main", dir.path(), None, 2, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_number, 1);
        assert!(results[0].content.contains("fn main"));
        assert!(!results[0].file.is_empty());
    }

    #[test]
    fn test_grep_search_invalid_regex() {
        let dir = TempDir::new().unwrap();
        let result = grep_search("[invalid", dir.path(), None, 2, 100);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid regex"));
    }

    #[test]
    fn test_grep_search_context_lines() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(
            &file_path,
            "line1\nline2\nline3\nMATCH\nline5\nline6\nline7\n",
        )
        .unwrap();

        let results = grep_search("MATCH", dir.path(), None, 2, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_number, 4);
        // Context should include 2 lines before and 2 lines after
        assert_eq!(results[0].context.len(), 5); // lines 2,3,4,5,6
        assert!(results[0].context[0].contains("line2"));
        assert!(results[0].context[4].contains("line6"));
    }

    #[test]
    fn test_grep_search_result_truncation() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("many_matches.txt");
        let content: String = (0..20).map(|i| format!("match_line_{}\n", i)).collect();
        std::fs::write(&file_path, content).unwrap();

        // Limit to 5 results
        let results = grep_search("match_line", dir.path(), None, 0, 5).unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_grep_search_skips_binary_files() {
        let dir = TempDir::new().unwrap();
        // Write a binary file
        let binary_path = dir.path().join("binary.bin");
        std::fs::write(&binary_path, &[0u8, 1, 2, 255, 0, 128]).unwrap();
        // Write a text file
        let text_path = dir.path().join("text.txt");
        std::fs::write(&text_path, "searchable text\n").unwrap();

        let results = grep_search("searchable", dir.path(), None, 0, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].file.contains("text.txt"));
    }

    #[test]
    fn test_grep_search_no_matches() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world\n").unwrap();

        let results = grep_search("nonexistent_pattern", dir.path(), None, 2, 100).unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_grep_tool_call_valid() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("code.rs");
        std::fs::write(&file_path, "fn hello() {}\nfn world() {}\n").unwrap();

        let tool = GrepTool::new();
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"pattern": "fn hello"}), &ctx, &progress)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.data["count"], 1);
        assert!(!result.data["truncated"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_grep_tool_call_invalid_regex() {
        let dir = TempDir::new().unwrap();
        let tool = GrepTool::new();
        let ctx = make_context(dir.path());
        let progress = NoopProgress;

        let result = tool
            .call(json!({"pattern": "[invalid"}), &ctx, &progress)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_grep_tool_validate_missing_pattern() {
        let tool = GrepTool::new();
        let dir = TempDir::new().unwrap();
        let ctx = make_context(dir.path());

        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_grep_tool_validate_empty_pattern() {
        let tool = GrepTool::new();
        let dir = TempDir::new().unwrap();
        let ctx = make_context(dir.path());

        let result = tool.validate_input(&json!({"pattern": ""}), &ctx).await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }
}
