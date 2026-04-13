use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::unified::UnifiedClient;
use crate::engine::query_engine::{EngineEvent, QueryEngine, QueryEngineConfig, ThinkingConfig};
use crate::tools::trait_def::*;

/// AgentTool — creates an independent sub-agent QueryEngine to execute a sub-task.
///
/// The sub-agent has its own message history and uses a read-only tool subset,
/// sharing the parent's API client.
pub struct AgentTool {
    api_client: Arc<UnifiedClient>,
    available_tools: Vec<Arc<dyn Tool>>,
    default_max_turns: u32,
}

impl AgentTool {
    pub fn new(
        api_client: Arc<UnifiedClient>,
        available_tools: Vec<Arc<dyn Tool>>,
    ) -> Self {
        Self {
            api_client,
            available_tools,
            default_max_turns: 10,
        }
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "AgentTool"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Agent"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "prompt": {
                    "type": "string",
                    "description": "子代理要执行的任务描述"
                },
                "max_turns": {
                    "type": "integer",
                    "description": "子代理最大工具调用轮次（默认 10）"
                },
                "model": {
                    "type": "string",
                    "description": "子代理使用的模型（默认继承主代理）"
                }
            })),
            required: Some(vec!["prompt".to_string()]),
            description: Some("创建子代理执行独立任务".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn prompt(&self) -> String {
        "Create an independent sub-agent to execute a task. The sub-agent has its own \
         conversation history and uses read-only tools. Useful for parallel research, \
         code analysis, or information gathering tasks."
            .to_string()
    }

    async fn validate_input(
        &self,
        input: &Value,
        _context: &ToolContext,
    ) -> ValidationResult {
        match input.get("prompt").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => ValidationResult::Ok,
            _ => ValidationResult::Invalid {
                message: "Missing or empty 'prompt' field. You must provide a 'prompt' string describing the task for the sub-agent. Example: {\"prompt\": \"search for files containing TODO\"}".to_string(),
                code: None,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let prompt = input["prompt"]
            .as_str()
            .ok_or(ToolError::ExecutionFailed("Missing prompt".into()))?;
        let max_turns = input["max_turns"]
            .as_u64()
            .unwrap_or(self.default_max_turns as u64) as u32;
        // Always use the engine's current model for sub-agents (ignore model in input)
        let model = context.model.clone();

        // Create sub-agent QueryEngine with independent message history
        let sub_engine_config = QueryEngineConfig {
            cwd: context.cwd.clone(),
            tools: self.available_tools.clone(),
            api_client: Arc::clone(&self.api_client),
            model,
            thinking_config: ThinkingConfig::Disabled,
            max_turns: Some(max_turns),
            max_budget_usd: None,
            verbose: false,
            custom_system_prompt: Some(
                "You are a sub-agent. Complete the given task using available tools. \
                 Be concise and focused."
                    .to_string(),
            ),
            append_system_prompt: None,
            session_id: None, // Sub-agent does not persist
            fallback_models: vec![],
            max_retries_per_model: 2,
        };

        let mut sub_engine = QueryEngine::new(sub_engine_config);
        let mut rx = sub_engine.submit_message(prompt.to_string()).await;

        // Collect sub-agent output
        let mut final_text = String::new();
        let mut total_cost = 0.0;

        while let Some(event) = rx.recv().await {
            match event {
                EngineEvent::AssistantChunk { content, .. } => {
                    final_text.push_str(&content);
                }
                EngineEvent::ToolUse {
                    tool_name,
                    tool_use_id,
                    ..
                } => {
                    progress
                        .send_progress(
                            &tool_use_id,
                            json!({
                                "sub_agent_tool": tool_name,
                            }),
                        )
                        .await;
                }
                EngineEvent::Result(result) => {
                    total_cost = result.total_cost_usd;
                    if let Some(text) = result.text {
                        final_text = text;
                    }
                    break;
                }
                EngineEvent::Error(err) => {
                    return Err(ToolError::ExecutionFailed(format!(
                        "Sub-agent error: {}",
                        err.message
                    )));
                }
                _ => {}
            }
        }

        Ok(ToolResult {
            data: json!({
                "result": final_text,
                "cost_usd": total_cost,
            }),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_api_client() -> Arc<UnifiedClient> {
        use crate::api::client::ApiClientConfig;
        Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
            api_key: "test-key".to_string(),
            base_url: None,
            max_retries: None,
        }))
    }

    #[test]
    fn test_agent_tool_name() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        assert_eq!(tool.name(), "AgentTool");
    }

    #[test]
    fn test_agent_tool_aliases() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        assert_eq!(tool.aliases(), vec!["Agent"]);
    }

    #[test]
    fn test_agent_tool_is_read_only() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_agent_tool_is_concurrency_safe() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_agent_tool_input_schema() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        let schema = tool.input_schema();
        assert_eq!(schema.schema_type, "object");
        assert_eq!(schema.required, Some(vec!["prompt".to_string()]));

        let props = schema.properties.unwrap();
        assert!(props.get("prompt").is_some());
        assert!(props.get("max_turns").is_some());
        assert!(props.get("model").is_some());
    }

    #[test]
    fn test_agent_tool_default_max_turns() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        assert_eq!(tool.default_max_turns, 10);
    }

    #[tokio::test]
    async fn test_agent_tool_validate_missing_prompt() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let ctx = ToolContext {
            cwd: std::path::PathBuf::from("/tmp"),
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
        };
        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_agent_tool_validate_empty_prompt() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let ctx = ToolContext {
            cwd: std::path::PathBuf::from("/tmp"),
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
        };
        let result = tool.validate_input(&json!({"prompt": ""}), &ctx).await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_agent_tool_validate_valid_prompt() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let ctx = ToolContext {
            cwd: std::path::PathBuf::from("/tmp"),
            model: "test".to_string(),
            abort_signal: Arc::new(rx),
        };
        let result = tool
            .validate_input(&json!({"prompt": "do something"}), &ctx)
            .await;
        assert!(matches!(result, ValidationResult::Ok));
    }

    #[test]
    fn test_agent_tool_prompt_description() {
        let tool = AgentTool::new(make_api_client(), vec![]);
        let prompt = tool.prompt();
        assert!(prompt.contains("sub-agent"));
    }
}
