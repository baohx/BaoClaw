use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::tools::trait_def::*;

/// Tool search — searches available tools by name, description, and aliases
pub struct ToolSearchTool {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolSearchTool {
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        Self { tools }
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearchTool"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["SearchTools"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "query": {
                    "type": "string",
                    "description": "Search keyword"
                }
            })),
            required: Some(vec!["query".to_string()]),
            description: Some("Search available tools by name and description".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn prompt(&self) -> String {
        "Search available tools by name, description, or aliases.".to_string()
    }

    async fn call(
        &self,
        input: Value,
        _context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'query'".to_string()))?
            .to_lowercase();

        let matches: Vec<Value> = self
            .tools
            .iter()
            .filter(|t| {
                t.name().to_lowercase().contains(&query)
                    || t.prompt().to_lowercase().contains(&query)
                    || t.aliases()
                        .iter()
                        .any(|a| a.to_lowercase().contains(&query))
            })
            .map(|t| {
                json!({
                    "name": t.name(),
                    "description": t.prompt(),
                    "aliases": t.aliases(),
                })
            })
            .collect();

        let count = matches.len();

        Ok(ToolResult {
            data: json!({
                "matches": matches,
                "count": count,
            }),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal mock tool for testing ToolSearchTool
    struct MockTool {
        tool_name: &'static str,
        tool_aliases: Vec<&'static str>,
        tool_prompt: &'static str,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            self.tool_name
        }
        fn aliases(&self) -> Vec<&str> {
            self.tool_aliases.clone()
        }
        fn input_schema(&self) -> JsonSchema {
            JsonSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                description: None,
            }
        }
        fn prompt(&self) -> String {
            self.tool_prompt.to_string()
        }
        async fn call(
            &self,
            _input: Value,
            _context: &ToolContext,
            _progress: &dyn ProgressSender,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                data: json!({}),
                is_error: false,
            })
        }
    }

    struct NoopProgress;
    #[async_trait]
    impl ProgressSender for NoopProgress {
        async fn send_progress(&self, _id: &str, _data: Value) {}
    }

    fn make_context() -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: std::path::PathBuf::from("/tmp"),
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
        }
    }

    fn make_tools() -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(MockTool {
                tool_name: "FileReadTool",
                tool_aliases: vec!["Read"],
                tool_prompt: "Read file contents from disk",
            }),
            Arc::new(MockTool {
                tool_name: "BashTool",
                tool_aliases: vec!["Shell"],
                tool_prompt: "Execute bash commands",
            }),
            Arc::new(MockTool {
                tool_name: "WebFetchTool",
                tool_aliases: vec!["Fetch"],
                tool_prompt: "Fetch web page content",
            }),
        ]
    }

    #[test]
    fn test_tool_name() {
        let tool = ToolSearchTool::new(vec![]);
        assert_eq!(tool.name(), "ToolSearchTool");
    }

    #[test]
    fn test_tool_aliases() {
        let tool = ToolSearchTool::new(vec![]);
        assert_eq!(tool.aliases(), vec!["SearchTools"]);
    }

    #[test]
    fn test_tool_is_read_only() {
        let tool = ToolSearchTool::new(vec![]);
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_tool_is_concurrency_safe() {
        let tool = ToolSearchTool::new(vec![]);
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[tokio::test]
    async fn test_search_by_name() {
        let tool = ToolSearchTool::new(make_tools());
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"query": "file"}), &ctx, &progress)
            .await
            .unwrap();

        assert_eq!(result.data["count"], 1);
        assert_eq!(result.data["matches"][0]["name"], "FileReadTool");
    }

    #[tokio::test]
    async fn test_search_case_insensitive() {
        let tool = ToolSearchTool::new(make_tools());
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"query": "BASH"}), &ctx, &progress)
            .await
            .unwrap();

        assert_eq!(result.data["count"], 1);
        assert_eq!(result.data["matches"][0]["name"], "BashTool");
    }

    #[tokio::test]
    async fn test_search_by_alias() {
        let tool = ToolSearchTool::new(make_tools());
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"query": "fetch"}), &ctx, &progress)
            .await
            .unwrap();

        assert_eq!(result.data["count"], 1);
        assert_eq!(result.data["matches"][0]["name"], "WebFetchTool");
    }

    #[tokio::test]
    async fn test_search_by_description() {
        let tool = ToolSearchTool::new(make_tools());
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"query": "bash commands"}), &ctx, &progress)
            .await
            .unwrap();

        assert_eq!(result.data["count"], 1);
        assert_eq!(result.data["matches"][0]["name"], "BashTool");
    }

    #[tokio::test]
    async fn test_search_no_results() {
        let tool = ToolSearchTool::new(make_tools());
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"query": "nonexistent"}), &ctx, &progress)
            .await
            .unwrap();

        assert_eq!(result.data["count"], 0);
        assert_eq!(result.data["matches"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_search_empty_tools_list() {
        let tool = ToolSearchTool::new(vec![]);
        let ctx = make_context();
        let progress = NoopProgress;

        let result = tool
            .call(json!({"query": "anything"}), &ctx, &progress)
            .await
            .unwrap();

        assert_eq!(result.data["count"], 0);
    }
}
