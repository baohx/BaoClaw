use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use super::trait_def::*;

/// Result of a single tool execution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    pub tool_use_id: String,
    pub tool_name: String,
    pub output: Value,
    pub is_error: bool,
}

/// A pending tool use from the LLM
#[derive(Clone, Debug)]
pub struct ToolUseRequest {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Execute a single tool following the pipeline: validate → permissions → call
pub async fn execute_tool(
    tool: &dyn Tool,
    request: &ToolUseRequest,
    context: &ToolContext,
    progress: &dyn ProgressSender,
) -> ToolExecutionResult {
    let tool_name = tool.name().to_string();
    let tool_use_id = request.id.clone();

    // Step 1: Validate input
    let validation = tool.validate_input(&request.input, context).await;
    if let ValidationResult::Invalid { message, .. } = validation {
        return ToolExecutionResult {
            tool_use_id,
            tool_name,
            output: Value::String(format!("Validation error: {}", message)),
            is_error: true,
        };
    }

    // Step 2: Check permissions
    let permission = tool.check_permissions(&request.input, context).await;
    if let ToolPermissionCheckResult::Deny { message } = permission {
        return ToolExecutionResult {
            tool_use_id,
            tool_name,
            output: Value::String(format!("Permission denied: {}", message)),
            is_error: true,
        };
    }

    // Step 3: Call the tool
    let call_result = tool.call(request.input.clone(), context, progress).await;
    match call_result {
        Ok(result) => {
            let max_size = tool.max_result_size_chars();
            let output = truncate_if_needed(result.data, max_size);
            ToolExecutionResult {
                tool_use_id,
                tool_name,
                output,
                is_error: result.is_error,
            }
        }
        Err(err) => ToolExecutionResult {
            tool_use_id,
            tool_name,
            output: Value::String(format!("Tool execution error: {}", err)),
            is_error: true,
        },
    }
}

/// Truncate result data if its serialized size exceeds max_size_chars.
fn truncate_if_needed(data: Value, max_size_chars: usize) -> Value {
    let serialized = serde_json::to_string(&data).unwrap_or_default();
    if serialized.len() <= max_size_chars {
        return data;
    }
    let truncated: String = serialized.chars().take(max_size_chars).collect();
    Value::String(format!(
        "{}\n\n[Result truncated: output exceeded {} characters]",
        truncated, max_size_chars
    ))
}

/// Execute multiple tools, running concurrency-safe tools in parallel
/// and non-concurrency-safe tools sequentially.
pub async fn execute_tools(
    tools: &[Arc<dyn Tool>],
    requests: &[ToolUseRequest],
    context: &ToolContext,
    progress: &dyn ProgressSender,
) -> Vec<ToolExecutionResult> {
    if requests.is_empty() {
        return vec![];
    }

    // Build (original_index, request, tool_ref) tuples
    let mut concurrent: Vec<(usize, &ToolUseRequest, &Arc<dyn Tool>)> = Vec::new();
    let mut sequential: Vec<(usize, &ToolUseRequest, &Arc<dyn Tool>)> = Vec::new();
    let mut not_found: Vec<(usize, &ToolUseRequest)> = Vec::new();

    for (idx, req) in requests.iter().enumerate() {
        match find_tool(tools, &req.name) {
            Some(tool) => {
                if tool.is_concurrency_safe(&req.input) {
                    concurrent.push((idx, req, tool));
                } else {
                    sequential.push((idx, req, tool));
                }
            }
            None => {
                not_found.push((idx, req));
            }
        }
    }

    let total = requests.len();
    let mut results: Vec<Option<ToolExecutionResult>> = vec![None; total];

    // Execute concurrent-safe tools in parallel
    if !concurrent.is_empty() {
        let futures: Vec<_> = concurrent
            .iter()
            .map(|(_, req, tool)| execute_tool(tool.as_ref(), req, context, progress))
            .collect();
        let concurrent_results = futures::future::join_all(futures).await;
        for ((idx, _, _), result) in concurrent.iter().zip(concurrent_results) {
            results[*idx] = Some(result);
        }
    }

    // Execute sequential tools one by one
    for (idx, req, tool) in &sequential {
        let result = execute_tool(tool.as_ref(), req, context, progress).await;
        results[*idx] = Some(result);
    }

    // Handle not-found tools
    for (idx, req) in &not_found {
        results[*idx] = Some(ToolExecutionResult {
            tool_use_id: req.id.clone(),
            tool_name: req.name.clone(),
            output: Value::String(format!("Tool '{}' not found", req.name)),
            is_error: true,
        });
    }

    // Unwrap all results (every slot should be filled)
    results.into_iter().map(|r| r.unwrap()).collect()
}

/// Find a tool by name (case-insensitive, also checks aliases)
pub fn find_tool<'a>(tools: &'a [Arc<dyn Tool>], name: &str) -> Option<&'a Arc<dyn Tool>> {
    let name_lower = name.to_ascii_lowercase();
    tools.iter().find(|t| {
        if t.name().to_ascii_lowercase() == name_lower {
            return true;
        }
        t.aliases()
            .iter()
            .any(|alias| alias.to_ascii_lowercase() == name_lower)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // --- Mock implementations ---

    struct MockProgressSender;

    #[async_trait::async_trait]
    impl ProgressSender for MockProgressSender {
        async fn send_progress(&self, _tool_use_id: &str, _data: Value) {}
    }

    /// A simple mock tool for testing
    struct MockTool {
        tool_name: String,
        tool_aliases: Vec<String>,
        concurrency_safe: bool,
        max_result_size: usize,
        validation_result: std::sync::Mutex<Option<ValidationResult>>,
        permission_result: std::sync::Mutex<Option<ToolPermissionCheckResult>>,
        call_result: std::sync::Mutex<Option<Result<ToolResult, ToolError>>>,
        call_count: AtomicUsize,
    }

    impl MockTool {
        fn new(name: &str) -> Self {
            Self {
                tool_name: name.to_string(),
                tool_aliases: vec![],
                concurrency_safe: false,
                max_result_size: 100_000,
                validation_result: std::sync::Mutex::new(None),
                permission_result: std::sync::Mutex::new(None),
                call_result: std::sync::Mutex::new(None),
                call_count: AtomicUsize::new(0),
            }
        }

        fn with_aliases(mut self, aliases: Vec<&str>) -> Self {
            self.tool_aliases = aliases.into_iter().map(String::from).collect();
            self
        }

        fn with_concurrency_safe(mut self, safe: bool) -> Self {
            self.concurrency_safe = safe;
            self
        }

        fn with_max_result_size(mut self, size: usize) -> Self {
            self.max_result_size = size;
            self
        }

        fn with_validation(self, result: ValidationResult) -> Self {
            *self.validation_result.lock().unwrap() = Some(result);
            self
        }

        fn with_permission(self, result: ToolPermissionCheckResult) -> Self {
            *self.permission_result.lock().unwrap() = Some(result);
            self
        }

        fn with_call_result(self, result: Result<ToolResult, ToolError>) -> Self {
            *self.call_result.lock().unwrap() = Some(result);
            self
        }
    }

    #[async_trait::async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.tool_name
        }

        fn aliases(&self) -> Vec<&str> {
            self.tool_aliases.iter().map(|s| s.as_str()).collect()
        }

        fn input_schema(&self) -> JsonSchema {
            JsonSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                description: None,
            }
        }

        fn is_concurrency_safe(&self, _input: &Value) -> bool {
            self.concurrency_safe
        }

        fn max_result_size_chars(&self) -> usize {
            self.max_result_size
        }

        async fn validate_input(
            &self,
            _input: &Value,
            _context: &ToolContext,
        ) -> ValidationResult {
            self.validation_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(ValidationResult::Ok)
        }

        async fn check_permissions(
            &self,
            _input: &Value,
            _context: &ToolContext,
        ) -> ToolPermissionCheckResult {
            self.permission_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(ToolPermissionCheckResult::Allow {
                    updated_input: Value::Null,
                })
        }

        async fn call(
            &self,
            _input: Value,
            _context: &ToolContext,
            _progress: &dyn ProgressSender,
        ) -> Result<ToolResult, ToolError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.call_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Ok(ToolResult {
                    data: json!({"result": "ok"}),
                    is_error: false,
                }))
        }

        fn prompt(&self) -> String {
            String::new()
        }
    }

    fn make_context() -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: PathBuf::from("/tmp"),
            model: "test-model".to_string(),
            abort_signal: Arc::new(rx),
        }
    }

    fn make_request(id: &str, name: &str) -> ToolUseRequest {
        ToolUseRequest {
            id: id.to_string(),
            name: name.to_string(),
            input: json!({}),
        }
    }

    // --- execute_tool tests ---

    #[tokio::test]
    async fn test_execute_tool_success() {
        let tool = MockTool::new("TestTool").with_call_result(Ok(ToolResult {
            data: json!({"hello": "world"}),
            is_error: false,
        }));
        let ctx = make_context();
        let progress = MockProgressSender;
        let request = make_request("req-1", "TestTool");

        let result = execute_tool(&tool, &request, &ctx, &progress).await;

        assert_eq!(result.tool_use_id, "req-1");
        assert_eq!(result.tool_name, "TestTool");
        assert_eq!(result.output, json!({"hello": "world"}));
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_execute_tool_validation_failure() {
        let tool = MockTool::new("TestTool").with_validation(ValidationResult::Invalid {
            message: "bad input".to_string(),
            code: None,
        });
        let ctx = make_context();
        let progress = MockProgressSender;
        let request = make_request("req-2", "TestTool");

        let result = execute_tool(&tool, &request, &ctx, &progress).await;

        assert!(result.is_error);
        assert!(result.output.as_str().unwrap().contains("Validation error"));
        assert!(result.output.as_str().unwrap().contains("bad input"));
        // call should not have been invoked
        assert_eq!(tool.call_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_execute_tool_permission_denied() {
        let tool =
            MockTool::new("TestTool").with_permission(ToolPermissionCheckResult::Deny {
                message: "not allowed".to_string(),
            });
        let ctx = make_context();
        let progress = MockProgressSender;
        let request = make_request("req-3", "TestTool");

        let result = execute_tool(&tool, &request, &ctx, &progress).await;

        assert!(result.is_error);
        assert!(result.output.as_str().unwrap().contains("Permission denied"));
        assert!(result.output.as_str().unwrap().contains("not allowed"));
        assert_eq!(tool.call_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_execute_tool_call_error() {
        let tool = MockTool::new("TestTool").with_call_result(Err(ToolError::ExecutionFailed(
            "something broke".to_string(),
        )));
        let ctx = make_context();
        let progress = MockProgressSender;
        let request = make_request("req-4", "TestTool");

        let result = execute_tool(&tool, &request, &ctx, &progress).await;

        assert!(result.is_error);
        assert!(result
            .output
            .as_str()
            .unwrap()
            .contains("Tool execution error"));
    }

    #[tokio::test]
    async fn test_execute_tool_truncates_large_result() {
        let large_data = "x".repeat(200);
        let tool = MockTool::new("TestTool")
            .with_max_result_size(50)
            .with_call_result(Ok(ToolResult {
                data: Value::String(large_data),
                is_error: false,
            }));
        let ctx = make_context();
        let progress = MockProgressSender;
        let request = make_request("req-5", "TestTool");

        let result = execute_tool(&tool, &request, &ctx, &progress).await;

        assert!(!result.is_error);
        let output_str = result.output.as_str().unwrap();
        assert!(output_str.contains("[Result truncated"));
    }

    // --- find_tool tests ---

    #[test]
    fn test_find_tool_by_name() {
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(MockTool::new("FileRead")),
            Arc::new(MockTool::new("Bash")),
        ];

        assert!(find_tool(&tools, "Bash").is_some());
        assert_eq!(find_tool(&tools, "Bash").unwrap().name(), "Bash");
    }

    #[test]
    fn test_find_tool_case_insensitive() {
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(MockTool::new("FileRead"))];

        assert!(find_tool(&tools, "fileread").is_some());
        assert!(find_tool(&tools, "FILEREAD").is_some());
        assert!(find_tool(&tools, "FileRead").is_some());
    }

    #[test]
    fn test_find_tool_by_alias() {
        let tools: Vec<Arc<dyn Tool>> =
            vec![Arc::new(MockTool::new("FileRead").with_aliases(vec!["Read", "FR"]))];

        assert!(find_tool(&tools, "Read").is_some());
        assert!(find_tool(&tools, "fr").is_some());
        assert!(find_tool(&tools, "FR").is_some());
    }

    #[test]
    fn test_find_tool_not_found() {
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(MockTool::new("FileRead"))];

        assert!(find_tool(&tools, "NonExistent").is_none());
    }

    // --- execute_tools tests ---

    #[tokio::test]
    async fn test_execute_tools_empty() {
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let requests: Vec<ToolUseRequest> = vec![];
        let ctx = make_context();
        let progress = MockProgressSender;

        let results = execute_tools(&tools, &requests, &ctx, &progress).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_execute_tools_not_found() {
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(MockTool::new("FileRead"))];
        let requests = vec![make_request("req-1", "NonExistent")];
        let ctx = make_context();
        let progress = MockProgressSender;

        let results = execute_tools(&tools, &requests, &ctx, &progress).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].is_error);
        assert!(results[0].output.as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_execute_tools_preserves_order() {
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(MockTool::new("ToolA").with_concurrency_safe(true)),
            Arc::new(MockTool::new("ToolB")),
            Arc::new(MockTool::new("ToolC").with_concurrency_safe(true)),
        ];
        let requests = vec![
            make_request("req-1", "ToolA"),
            make_request("req-2", "ToolB"),
            make_request("req-3", "ToolC"),
        ];
        let ctx = make_context();
        let progress = MockProgressSender;

        let results = execute_tools(&tools, &requests, &ctx, &progress).await;

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].tool_use_id, "req-1");
        assert_eq!(results[0].tool_name, "ToolA");
        assert_eq!(results[1].tool_use_id, "req-2");
        assert_eq!(results[1].tool_name, "ToolB");
        assert_eq!(results[2].tool_use_id, "req-3");
        assert_eq!(results[2].tool_name, "ToolC");
    }

    #[tokio::test]
    async fn test_execute_tools_mixed_concurrent_sequential() {
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(MockTool::new("ConcurrentTool").with_concurrency_safe(true)),
            Arc::new(MockTool::new("SequentialTool")),
        ];
        let requests = vec![
            make_request("req-1", "ConcurrentTool"),
            make_request("req-2", "SequentialTool"),
        ];
        let ctx = make_context();
        let progress = MockProgressSender;

        let results = execute_tools(&tools, &requests, &ctx, &progress).await;

        assert_eq!(results.len(), 2);
        assert!(!results[0].is_error);
        assert!(!results[1].is_error);
    }

    // --- truncate_if_needed tests ---

    #[test]
    fn test_truncate_if_needed_small() {
        let data = json!("hello");
        let result = truncate_if_needed(data.clone(), 1000);
        assert_eq!(result, data);
    }

    #[test]
    fn test_truncate_if_needed_large() {
        let data = Value::String("x".repeat(100));
        let result = truncate_if_needed(data, 20);
        let s = result.as_str().unwrap();
        assert!(s.contains("[Result truncated"));
    }
}
