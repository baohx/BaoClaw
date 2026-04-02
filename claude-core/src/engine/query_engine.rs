use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use crate::api::client::{AnthropicClient, ApiStreamEvent, CreateMessageRequest};
use crate::models::message::{ContentBlock, Message, MessageContent, ApiAssistantMessage, ApiUserMessage, Usage};
use crate::tools::executor::{execute_tools, ToolExecutionResult, ToolUseRequest};
use crate::tools::trait_def::{ProgressSender, Tool, ToolContext};

/// Constant representing zero usage, useful for initialization.
pub const EMPTY_USAGE: Usage = Usage {
    input_tokens: 0,
    output_tokens: 0,
    cache_creation_input_tokens: None,
    cache_read_input_tokens: None,
};

/// Configuration for the QueryEngine.
pub struct QueryEngineConfig {
    pub cwd: PathBuf,
    pub tools: Vec<Arc<dyn Tool>>,
    pub api_client: Arc<AnthropicClient>,
    pub model: String,
    pub thinking_config: ThinkingConfig,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub verbose: bool,
    pub custom_system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
}

/// Thinking mode configuration for the LLM.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum ThinkingConfig {
    #[serde(rename = "disabled")]
    Disabled,
    #[serde(rename = "adaptive")]
    Adaptive,
    #[serde(rename = "enabled")]
    Enabled { budget_tokens: u32 },
}

/// Events yielded by the QueryEngine during message processing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EngineEvent {
    #[serde(rename = "assistant_chunk")]
    AssistantChunk {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        tool_name: String,
        input: Value,
        tool_use_id: String,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        output: Value,
        is_error: bool,
    },
    #[serde(rename = "permission_request")]
    PermissionRequest {
        tool_name: String,
        input: Value,
        tool_use_id: String,
    },
    #[serde(rename = "progress")]
    Progress {
        tool_use_id: String,
        data: Value,
    },
    #[serde(rename = "state_update")]
    StateUpdate { patch: Value },
    #[serde(rename = "result")]
    Result(QueryResult),
    #[serde(rename = "error")]
    Error(EngineError),
}

/// Result of a completed query.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub status: QueryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    pub total_cost_usd: f64,
    pub usage: Usage,
    pub num_turns: u32,
    pub duration_ms: u64,
}

/// Status of a completed query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum QueryStatus {
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "max_turns")]
    MaxTurns,
    #[serde(rename = "aborted")]
    Aborted,
    #[serde(rename = "error")]
    Error,
}

/// Error information from the engine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// The core QueryEngine that orchestrates LLM calls, tool execution, and message management.
pub struct QueryEngine {
    config: QueryEngineConfig,
    messages: Vec<Message>,
    abort_tx: watch::Sender<bool>,
    abort_rx: watch::Receiver<bool>,
    total_usage: Usage,
}

impl QueryEngine {
    /// Create a new QueryEngine with the given configuration.
    pub fn new(config: QueryEngineConfig) -> Self {
        let (abort_tx, abort_rx) = watch::channel(false);
        Self {
            config,
            messages: Vec::new(),
            abort_tx,
            abort_rx,
            total_usage: EMPTY_USAGE,
        }
    }

    /// Signal the engine to abort the current operation.
    pub fn abort(&self) {
        let _ = self.abort_tx.send(true);
    }

    /// Check whether the engine has been aborted.
    pub fn is_aborted(&self) -> bool {
        *self.abort_rx.borrow()
    }

    /// Get a reference to the conversation message history.
    pub fn get_messages(&self) -> &[Message] {
        &self.messages
    }

    /// Get a reference to the accumulated usage statistics.
    pub fn get_usage(&self) -> &Usage {
        &self.total_usage
    }

    /// Submit a user message and process the response loop.
    /// Returns a receiver that yields EngineEvent items.
    pub async fn submit_message(
        &mut self,
        prompt: String,
    ) -> mpsc::Receiver<EngineEvent> {
        let (tx, rx) = mpsc::channel(256);

        // Build the user message and append to history
        let user_msg = Message {
            uuid: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: MessageContent::User {
                message: ApiUserMessage {
                    role: "user".to_string(),
                    content: Value::String(prompt),
                },
                is_meta: false,
                tool_use_result: None,
            },
        };
        self.messages.push(user_msg);

        // Build the config for the spawned loop
        let loop_config = QueryLoopConfig {
            api_client: Arc::clone(&self.config.api_client),
            tools: self.config.tools.clone(),
            model: self.config.model.clone(),
            max_turns: self.config.max_turns,
            cwd: self.config.cwd.clone(),
            custom_system_prompt: self.config.custom_system_prompt.clone(),
            append_system_prompt: self.config.append_system_prompt.clone(),
            abort_rx: self.abort_rx.clone(),
        };

        let messages = self.messages.clone();

        tokio::spawn(async move {
            run_query_loop(messages, loop_config, tx).await;
        });

        rx
    }
}

/// A no-op progress sender for use in the query loop when no progress reporting is needed.
pub struct NoopProgressSender;

#[async_trait::async_trait]
impl ProgressSender for NoopProgressSender {
    async fn send_progress(&self, _tool_use_id: &str, _data: Value) {}
}

/// Configuration extracted from QueryEngine for the spawned query loop task.
struct QueryLoopConfig {
    api_client: Arc<AnthropicClient>,
    tools: Vec<Arc<dyn Tool>>,
    model: String,
    max_turns: Option<u32>,
    cwd: PathBuf,
    custom_system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    abort_rx: watch::Receiver<bool>,
}

impl QueryLoopConfig {
    fn is_aborted(&self) -> bool {
        *self.abort_rx.borrow()
    }
}

/// The core query loop that calls the LLM, processes tool uses, and loops until done.
async fn run_query_loop(
    mut messages: Vec<Message>,
    config: QueryLoopConfig,
    tx: mpsc::Sender<EngineEvent>,
) {
    let start_time = std::time::Instant::now();
    let mut turn_count = 0u32;
    let mut total_usage = EMPTY_USAGE;

    loop {
        // Check abort
        if config.is_aborted() {
            let _ = tx.send(EngineEvent::Result(QueryResult {
                status: QueryStatus::Aborted,
                text: None,
                stop_reason: None,
                total_cost_usd: 0.0,
                usage: total_usage,
                num_turns: turn_count,
                duration_ms: start_time.elapsed().as_millis() as u64,
            })).await;
            return;
        }

        // Check max_turns
        if let Some(max) = config.max_turns {
            if turn_count >= max {
                let _ = tx.send(EngineEvent::Result(QueryResult {
                    status: QueryStatus::MaxTurns,
                    text: None,
                    stop_reason: None,
                    total_cost_usd: 0.0,
                    usage: total_usage,
                    num_turns: turn_count,
                    duration_ms: start_time.elapsed().as_millis() as u64,
                })).await;
                return;
            }
        }

        // Build API request
        let request = build_api_request(&messages, &config);

        // Call LLM API (streaming)
        let stream_result = config.api_client.create_message_stream(request).await;
        let mut stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(EngineEvent::Error(EngineError {
                    code: "api_error".to_string(),
                    message: format!("{}", e),
                    details: None,
                })).await;
                return;
            }
        };

        // Process SSE stream events, accumulating content blocks
        let mut assistant_content_blocks: Vec<ContentBlock> = Vec::new();
        let mut current_text = String::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_input_json = String::new();
        let mut stop_reason: Option<String> = None;
        // Track what kind of block we're in: "text", "tool_use", or ""
        let mut current_block_type = String::new();

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => match event {
                    ApiStreamEvent::ContentBlockStart { content_block, .. } => {
                        let block_type = content_block.get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        current_block_type = block_type.to_string();
                        match block_type {
                            "text" => {
                                current_text = String::new();
                            }
                            "tool_use" => {
                                current_tool_id = content_block.get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                current_tool_name = content_block.get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                current_tool_input_json = String::new();
                            }
                            _ => {}
                        }
                    }
                    ApiStreamEvent::ContentBlockDelta { delta, .. } => {
                        let delta_type = delta.get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        match delta_type {
                            "text_delta" => {
                                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                    current_text.push_str(text);
                                    // Emit AssistantChunk
                                    let _ = tx.send(EngineEvent::AssistantChunk {
                                        content: text.to_string(),
                                        tool_use_id: None,
                                    }).await;
                                }
                            }
                            "input_json_delta" => {
                                if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                    current_tool_input_json.push_str(partial);
                                }
                            }
                            _ => {}
                        }
                    }
                    ApiStreamEvent::ContentBlockStop { .. } => {
                        match current_block_type.as_str() {
                            "text" => {
                                if !current_text.is_empty() {
                                    assistant_content_blocks.push(ContentBlock::Text {
                                        text: current_text.clone(),
                                    });
                                }
                            }
                            "tool_use" => {
                                let input: Value = serde_json::from_str(&current_tool_input_json)
                                    .unwrap_or(Value::Object(serde_json::Map::new()));
                                assistant_content_blocks.push(ContentBlock::ToolUse {
                                    id: current_tool_id.clone(),
                                    name: current_tool_name.clone(),
                                    input: input.clone(),
                                });
                            }
                            _ => {}
                        }
                        current_block_type.clear();
                    }
                    ApiStreamEvent::MessageDelta { delta, usage, .. } => {
                        if let Some(sr) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                            stop_reason = Some(sr.to_string());
                        }
                        accumulate_usage(&mut total_usage, &usage);
                    }
                    ApiStreamEvent::MessageStart { message } => {
                        // Extract usage from message_start if present
                        if let Some(usage_val) = message.get("usage") {
                            accumulate_usage(&mut total_usage, usage_val);
                        }
                    }
                    ApiStreamEvent::MessageStop => {
                        break;
                    }
                    ApiStreamEvent::Error { error } => {
                        let _ = tx.send(EngineEvent::Error(EngineError {
                            code: error.error_type,
                            message: error.message,
                            details: None,
                        })).await;
                        return;
                    }
                    ApiStreamEvent::Ping => {}
                },
                Err(e) => {
                    let _ = tx.send(EngineEvent::Error(EngineError {
                        code: "stream_error".to_string(),
                        message: format!("{}", e),
                        details: None,
                    })).await;
                    return;
                }
            }
        }

        // Build assistant message and append to history
        let assistant_msg = Message {
            uuid: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".to_string(),
                    content: assistant_content_blocks.clone(),
                    stop_reason: stop_reason.clone(),
                    usage: None,
                },
                cost_usd: 0.0,
                duration_ms: 0,
            },
        };
        messages.push(assistant_msg);

        // Check for tool_use blocks
        let tool_uses = extract_tool_uses(&assistant_content_blocks);

        if tool_uses.is_empty() {
            // No tools → query complete
            let text = extract_text(&assistant_content_blocks);
            let _ = tx.send(EngineEvent::Result(QueryResult {
                status: QueryStatus::Complete,
                text,
                stop_reason,
                total_cost_usd: 0.0,
                usage: total_usage,
                num_turns: turn_count,
                duration_ms: start_time.elapsed().as_millis() as u64,
            })).await;
            return;
        }

        // Emit ToolUse events
        for tu in &tool_uses {
            let _ = tx.send(EngineEvent::ToolUse {
                tool_name: tu.name.clone(),
                input: tu.input.clone(),
                tool_use_id: tu.id.clone(),
            }).await;
        }

        // Execute tools using the executor
        let tool_context = ToolContext {
            cwd: config.cwd.clone(),
            model: config.model.clone(),
            abort_signal: Arc::new(config.abort_rx.clone()),
        };
        let progress = NoopProgressSender;
        let tool_results = execute_tools(&config.tools, &tool_uses, &tool_context, &progress).await;

        // Emit ToolResult events
        for result in &tool_results {
            let _ = tx.send(EngineEvent::ToolResult {
                tool_use_id: result.tool_use_id.clone(),
                output: result.output.clone(),
                is_error: result.is_error,
            }).await;
        }

        // Build tool result user message and append to messages
        let tool_result_msg = build_tool_result_message(&tool_results);
        messages.push(tool_result_msg);

        turn_count += 1;
    }
}

/// Build an API request from the current messages and config.
fn build_api_request(messages: &[Message], config: &QueryLoopConfig) -> CreateMessageRequest {
    // Convert messages to API format
    let api_messages: Vec<Value> = messages.iter().filter_map(|msg| {
        match &msg.content {
            MessageContent::User { message, .. } => {
                Some(serde_json::json!({
                    "role": message.role,
                    "content": message.content,
                }))
            }
            MessageContent::Assistant { message, .. } => {
                let content_value = serde_json::to_value(&message.content).unwrap_or(Value::Array(vec![]));
                Some(serde_json::json!({
                    "role": message.role,
                    "content": content_value,
                }))
            }
            _ => None,
        }
    }).collect();

    // Build system prompt
    let system = build_system_prompt(config);

    // Build tools list
    let tools: Option<Vec<Value>> = if config.tools.is_empty() {
        None
    } else {
        Some(config.tools.iter().map(|t| {
            let schema = t.input_schema();
            serde_json::json!({
                "name": t.name(),
                "description": t.prompt(),
                "input_schema": schema,
            })
        }).collect())
    };

    CreateMessageRequest {
        model: config.model.clone(),
        messages: api_messages,
        system,
        tools,
        max_tokens: 16384,
        stream: true,
        thinking: None,
        metadata: None,
    }
}

/// Build the system prompt from config.
fn build_system_prompt(config: &QueryLoopConfig) -> Option<Vec<Value>> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(custom) = &config.custom_system_prompt {
        parts.push(custom.clone());
    } else {
        parts.push("You are a helpful AI coding assistant.".to_string());
    }

    if let Some(append) = &config.append_system_prompt {
        parts.push(append.clone());
    }

    if parts.is_empty() {
        None
    } else {
        let combined = parts.join("\n\n");
        Some(vec![serde_json::json!({
            "type": "text",
            "text": combined,
        })])
    }
}

/// Extract tool use requests from assistant content blocks.
fn extract_tool_uses(content_blocks: &[ContentBlock]) -> Vec<ToolUseRequest> {
    content_blocks.iter().filter_map(|block| {
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                Some(ToolUseRequest {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                })
            }
            _ => None,
        }
    }).collect()
}

/// Extract text content from assistant content blocks.
fn extract_text(content_blocks: &[ContentBlock]) -> Option<String> {
    let texts: Vec<&str> = content_blocks.iter().filter_map(|block| {
        match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        }
    }).collect();

    if texts.is_empty() {
        None
    } else {
        Some(texts.join(""))
    }
}

/// Build a user message containing tool results.
fn build_tool_result_message(results: &[ToolExecutionResult]) -> Message {
    let content_blocks: Vec<Value> = results.iter().map(|r| {
        serde_json::json!({
            "type": "tool_result",
            "tool_use_id": r.tool_use_id,
            "content": r.output,
            "is_error": r.is_error,
        })
    }).collect();

    Message {
        uuid: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content: MessageContent::User {
            message: ApiUserMessage {
                role: "user".to_string(),
                content: Value::Array(content_blocks),
            },
            is_meta: false,
            tool_use_result: None,
        },
    }
}

/// Accumulate usage from a delta value into the total.
fn accumulate_usage(total: &mut Usage, delta: &Value) {
    if let Some(input) = delta.get("input_tokens").and_then(|v| v.as_u64()) {
        total.input_tokens += input;
    }
    if let Some(output) = delta.get("output_tokens").and_then(|v| v.as_u64()) {
        total.output_tokens += output;
    }
    if let Some(cache_create) = delta.get("cache_creation_input_tokens").and_then(|v| v.as_u64()) {
        *total.cache_creation_input_tokens.get_or_insert(0) += cache_create;
    }
    if let Some(cache_read) = delta.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
        *total.cache_read_input_tokens.get_or_insert(0) += cache_read;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::{AnthropicClient, ApiClientConfig};
    use crate::models::message::ContentBlock;
    use serde_json::json;

    fn make_config() -> QueryEngineConfig {
        let api_client = Arc::new(AnthropicClient::new(ApiClientConfig {
            api_key: "test-key".to_string(),
            base_url: None,
            max_retries: None,
        }));
        QueryEngineConfig {
            cwd: PathBuf::from("/tmp"),
            tools: vec![],
            api_client,
            model: "claude-sonnet-4-20250514".to_string(),
            thinking_config: ThinkingConfig::Disabled,
            max_turns: None,
            max_budget_usd: None,
            verbose: false,
            custom_system_prompt: None,
            append_system_prompt: None,
        }
    }

    // --- QueryEngine construction tests ---

    #[test]
    fn test_new_engine_has_empty_messages() {
        let engine = QueryEngine::new(make_config());
        assert!(engine.get_messages().is_empty());
    }

    #[test]
    fn test_new_engine_has_zero_usage() {
        let engine = QueryEngine::new(make_config());
        let usage = engine.get_usage();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert!(usage.cache_creation_input_tokens.is_none());
        assert!(usage.cache_read_input_tokens.is_none());
    }

    #[test]
    fn test_new_engine_not_aborted() {
        let engine = QueryEngine::new(make_config());
        assert!(!engine.is_aborted());
    }

    // --- Abort tests ---

    #[test]
    fn test_abort_sets_flag() {
        let engine = QueryEngine::new(make_config());
        assert!(!engine.is_aborted());
        engine.abort();
        assert!(engine.is_aborted());
    }

    #[test]
    fn test_abort_is_idempotent() {
        let engine = QueryEngine::new(make_config());
        engine.abort();
        engine.abort();
        assert!(engine.is_aborted());
    }

    // --- EMPTY_USAGE constant test ---

    #[test]
    fn test_empty_usage_constant() {
        assert_eq!(EMPTY_USAGE.input_tokens, 0);
        assert_eq!(EMPTY_USAGE.output_tokens, 0);
        assert!(EMPTY_USAGE.cache_creation_input_tokens.is_none());
        assert!(EMPTY_USAGE.cache_read_input_tokens.is_none());
    }

    // --- EngineEvent serialization tests ---

    #[test]
    fn test_serialize_assistant_chunk() {
        let event = EngineEvent::AssistantChunk {
            content: "Hello".to_string(),
            tool_use_id: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "assistant_chunk");
        assert_eq!(json["content"], "Hello");
        assert!(json.get("tool_use_id").is_none());
    }

    #[test]
    fn test_serialize_assistant_chunk_with_tool_use_id() {
        let event = EngineEvent::AssistantChunk {
            content: "data".to_string(),
            tool_use_id: Some("tu_123".to_string()),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "assistant_chunk");
        assert_eq!(json["tool_use_id"], "tu_123");
    }

    #[test]
    fn test_serialize_tool_use() {
        let event = EngineEvent::ToolUse {
            tool_name: "Bash".to_string(),
            input: json!({"command": "ls"}),
            tool_use_id: "tu_1".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "tool_use");
        assert_eq!(json["tool_name"], "Bash");
        assert_eq!(json["input"]["command"], "ls");
        assert_eq!(json["tool_use_id"], "tu_1");
    }

    #[test]
    fn test_serialize_tool_result() {
        let event = EngineEvent::ToolResult {
            tool_use_id: "tu_1".to_string(),
            output: json!({"stdout": "file.txt"}),
            is_error: false,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "tool_result");
        assert_eq!(json["tool_use_id"], "tu_1");
        assert!(!json["is_error"].as_bool().unwrap());
    }

    #[test]
    fn test_serialize_permission_request() {
        let event = EngineEvent::PermissionRequest {
            tool_name: "FileWrite".to_string(),
            input: json!({"path": "/tmp/test.txt"}),
            tool_use_id: "tu_2".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "permission_request");
        assert_eq!(json["tool_name"], "FileWrite");
    }

    #[test]
    fn test_serialize_progress() {
        let event = EngineEvent::Progress {
            tool_use_id: "tu_3".to_string(),
            data: json!({"percent": 50}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "progress");
        assert_eq!(json["data"]["percent"], 50);
    }

    #[test]
    fn test_serialize_state_update() {
        let event = EngineEvent::StateUpdate {
            patch: json!({"path": "/tasks/b12345678", "op": "replace", "value": "running"}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "state_update");
    }

    #[test]
    fn test_serialize_result_event() {
        let event = EngineEvent::Result(QueryResult {
            status: QueryStatus::Complete,
            text: Some("Done!".to_string()),
            stop_reason: Some("end_turn".to_string()),
            total_cost_usd: 0.005,
            usage: EMPTY_USAGE,
            num_turns: 3,
            duration_ms: 1500,
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "result");
        assert_eq!(json["status"], "complete");
        assert_eq!(json["text"], "Done!");
        assert_eq!(json["num_turns"], 3);
    }

    #[test]
    fn test_serialize_error_event() {
        let event = EngineEvent::Error(EngineError {
            code: "api_error".to_string(),
            message: "Rate limited".to_string(),
            details: None,
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["code"], "api_error");
        assert_eq!(json["message"], "Rate limited");
        assert!(json.get("details").is_none());
    }

    // --- EngineEvent deserialization round-trip tests ---

    #[test]
    fn test_engine_event_roundtrip_tool_use() {
        let event = EngineEvent::ToolUse {
            tool_name: "Bash".to_string(),
            input: json!({"command": "echo hello"}),
            tool_use_id: "tu_rt".to_string(),
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let deserialized: EngineEvent = serde_json::from_str(&json_str).unwrap();
        match deserialized {
            EngineEvent::ToolUse {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_use_id, "tu_rt");
            }
            _ => panic!("Expected ToolUse"),
        }
    }

    // --- QueryStatus tests ---

    #[test]
    fn test_query_status_serialization() {
        assert_eq!(
            serde_json::to_value(QueryStatus::Complete).unwrap(),
            json!("complete")
        );
        assert_eq!(
            serde_json::to_value(QueryStatus::MaxTurns).unwrap(),
            json!("max_turns")
        );
        assert_eq!(
            serde_json::to_value(QueryStatus::Aborted).unwrap(),
            json!("aborted")
        );
        assert_eq!(
            serde_json::to_value(QueryStatus::Error).unwrap(),
            json!("error")
        );
    }

    #[test]
    fn test_query_status_equality() {
        assert_eq!(QueryStatus::Complete, QueryStatus::Complete);
        assert_ne!(QueryStatus::Complete, QueryStatus::Error);
    }

    // --- ThinkingConfig tests ---

    #[test]
    fn test_thinking_config_serialization() {
        let disabled = ThinkingConfig::Disabled;
        let json = serde_json::to_value(&disabled).unwrap();
        assert_eq!(json["mode"], "disabled");

        let adaptive = ThinkingConfig::Adaptive;
        let json = serde_json::to_value(&adaptive).unwrap();
        assert_eq!(json["mode"], "adaptive");

        let enabled = ThinkingConfig::Enabled {
            budget_tokens: 1024,
        };
        let json = serde_json::to_value(&enabled).unwrap();
        assert_eq!(json["mode"], "enabled");
        assert_eq!(json["budget_tokens"], 1024);
    }

    #[test]
    fn test_thinking_config_roundtrip() {
        let enabled = ThinkingConfig::Enabled {
            budget_tokens: 2048,
        };
        let json_str = serde_json::to_string(&enabled).unwrap();
        let deserialized: ThinkingConfig = serde_json::from_str(&json_str).unwrap();
        match deserialized {
            ThinkingConfig::Enabled { budget_tokens } => assert_eq!(budget_tokens, 2048),
            _ => panic!("Expected Enabled"),
        }
    }

    // --- QueryResult optional field tests ---

    #[test]
    fn test_query_result_without_optional_fields() {
        let result = QueryResult {
            status: QueryStatus::Aborted,
            text: None,
            stop_reason: None,
            total_cost_usd: 0.0,
            usage: EMPTY_USAGE,
            num_turns: 0,
            duration_ms: 0,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("text").is_none());
        assert!(json.get("stop_reason").is_none());
    }

    // --- Helper function tests ---

    #[test]
    fn test_extract_tool_uses_empty() {
        let blocks: Vec<ContentBlock> = vec![];
        let result = extract_tool_uses(&blocks);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_tool_uses_text_only() {
        let blocks = vec![
            ContentBlock::Text { text: "Hello world".to_string() },
        ];
        let result = extract_tool_uses(&blocks);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_tool_uses_with_tools() {
        let blocks = vec![
            ContentBlock::Text { text: "Let me run that.".to_string() },
            ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: json!({"command": "ls"}),
            },
            ContentBlock::ToolUse {
                id: "tu_2".to_string(),
                name: "FileRead".to_string(),
                input: json!({"path": "/tmp/test.txt"}),
            },
        ];
        let result = extract_tool_uses(&blocks);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "tu_1");
        assert_eq!(result[0].name, "Bash");
        assert_eq!(result[1].id, "tu_2");
        assert_eq!(result[1].name, "FileRead");
    }

    #[test]
    fn test_extract_text_empty() {
        let blocks: Vec<ContentBlock> = vec![];
        assert!(extract_text(&blocks).is_none());
    }

    #[test]
    fn test_extract_text_single() {
        let blocks = vec![
            ContentBlock::Text { text: "Hello".to_string() },
        ];
        assert_eq!(extract_text(&blocks), Some("Hello".to_string()));
    }

    #[test]
    fn test_extract_text_multiple() {
        let blocks = vec![
            ContentBlock::Text { text: "Hello ".to_string() },
            ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: json!({}),
            },
            ContentBlock::Text { text: "world".to_string() },
        ];
        assert_eq!(extract_text(&blocks), Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_text_tool_only() {
        let blocks = vec![
            ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: json!({}),
            },
        ];
        assert!(extract_text(&blocks).is_none());
    }

    #[test]
    fn test_accumulate_usage_basic() {
        let mut total = EMPTY_USAGE;
        let delta = json!({"input_tokens": 100, "output_tokens": 50});
        accumulate_usage(&mut total, &delta);
        assert_eq!(total.input_tokens, 100);
        assert_eq!(total.output_tokens, 50);
    }

    #[test]
    fn test_accumulate_usage_multiple() {
        let mut total = EMPTY_USAGE;
        accumulate_usage(&mut total, &json!({"input_tokens": 100, "output_tokens": 50}));
        accumulate_usage(&mut total, &json!({"input_tokens": 200, "output_tokens": 30}));
        assert_eq!(total.input_tokens, 300);
        assert_eq!(total.output_tokens, 80);
    }

    #[test]
    fn test_accumulate_usage_with_cache() {
        let mut total = EMPTY_USAGE;
        accumulate_usage(&mut total, &json!({
            "input_tokens": 10,
            "output_tokens": 5,
            "cache_creation_input_tokens": 20,
            "cache_read_input_tokens": 30
        }));
        assert_eq!(total.input_tokens, 10);
        assert_eq!(total.output_tokens, 5);
        assert_eq!(total.cache_creation_input_tokens, Some(20));
        assert_eq!(total.cache_read_input_tokens, Some(30));
    }

    #[test]
    fn test_accumulate_usage_empty_delta() {
        let mut total = EMPTY_USAGE;
        accumulate_usage(&mut total, &json!({}));
        assert_eq!(total.input_tokens, 0);
        assert_eq!(total.output_tokens, 0);
    }

    #[test]
    fn test_build_tool_result_message() {
        use crate::tools::executor::ToolExecutionResult;
        let results = vec![
            ToolExecutionResult {
                tool_use_id: "tu_1".to_string(),
                tool_name: "Bash".to_string(),
                output: json!({"stdout": "hello"}),
                is_error: false,
            },
            ToolExecutionResult {
                tool_use_id: "tu_2".to_string(),
                tool_name: "FileRead".to_string(),
                output: json!("Permission denied"),
                is_error: true,
            },
        ];
        let msg = build_tool_result_message(&results);
        match &msg.content {
            MessageContent::User { message, .. } => {
                assert_eq!(message.role, "user");
                let content = message.content.as_array().unwrap();
                assert_eq!(content.len(), 2);
                assert_eq!(content[0]["tool_use_id"], "tu_1");
                assert!(!content[0]["is_error"].as_bool().unwrap());
                assert_eq!(content[1]["tool_use_id"], "tu_2");
                assert!(content[1]["is_error"].as_bool().unwrap());
            }
            _ => panic!("Expected User message"),
        }
    }

    #[test]
    fn test_build_system_prompt_default() {
        let (_abort_tx, abort_rx) = watch::channel(false);
        let config = QueryLoopConfig {
            api_client: Arc::new(AnthropicClient::new(ApiClientConfig {
                api_key: "test".to_string(),
                base_url: None,
                max_retries: None,
            })),
            tools: vec![],
            model: "test".to_string(),
            max_turns: None,
            cwd: PathBuf::from("/tmp"),
            custom_system_prompt: None,
            append_system_prompt: None,
            abort_rx,
        };
        let system = build_system_prompt(&config);
        assert!(system.is_some());
        let blocks = system.unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0]["text"].as_str().unwrap().contains("helpful AI coding assistant"));
    }

    #[test]
    fn test_build_system_prompt_custom() {
        let (_abort_tx, abort_rx) = watch::channel(false);
        let config = QueryLoopConfig {
            api_client: Arc::new(AnthropicClient::new(ApiClientConfig {
                api_key: "test".to_string(),
                base_url: None,
                max_retries: None,
            })),
            tools: vec![],
            model: "test".to_string(),
            max_turns: None,
            cwd: PathBuf::from("/tmp"),
            custom_system_prompt: Some("You are a Rust expert.".to_string()),
            append_system_prompt: Some("Be concise.".to_string()),
            abort_rx,
        };
        let system = build_system_prompt(&config);
        assert!(system.is_some());
        let text = system.unwrap()[0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Rust expert"));
        assert!(text.contains("Be concise"));
    }

    #[test]
    fn test_build_api_request_basic() {
        let (_abort_tx, abort_rx) = watch::channel(false);
        let config = QueryLoopConfig {
            api_client: Arc::new(AnthropicClient::new(ApiClientConfig {
                api_key: "test".to_string(),
                base_url: None,
                max_retries: None,
            })),
            tools: vec![],
            model: "claude-sonnet-4-20250514".to_string(),
            max_turns: None,
            cwd: PathBuf::from("/tmp"),
            custom_system_prompt: None,
            append_system_prompt: None,
            abort_rx,
        };
        let messages = vec![
            Message {
                uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                timestamp: "2024-01-15T10:30:00Z".to_string(),
                content: MessageContent::User {
                    message: ApiUserMessage {
                        role: "user".to_string(),
                        content: Value::String("Hello".to_string()),
                    },
                    is_meta: false,
                    tool_use_result: None,
                },
            },
        ];
        let request = build_api_request(&messages, &config);
        assert_eq!(request.model, "claude-sonnet-4-20250514");
        assert!(request.stream);
        assert_eq!(request.messages.len(), 1);
        assert!(request.tools.is_none());
        assert!(request.system.is_some());
    }

    #[test]
    fn test_noop_progress_sender() {
        // Just verify it compiles and can be used
        let _sender = NoopProgressSender;
    }
}
