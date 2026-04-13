use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use crate::api::client::{ApiError, ApiStreamEvent, CreateMessageRequest};
use crate::api::unified::UnifiedClient;
use crate::api::fallback::{FallbackAction, FallbackController};
use crate::config::BaoclawConfig;
use crate::engine::cost_tracker::CostTracker;
use crate::engine::git_info::{get_git_info, GitInfo};
use crate::engine::transcript::{TranscriptEntry, TranscriptEntryType, TranscriptWriter};
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
    pub api_client: Arc<UnifiedClient>,
    pub model: String,
    pub thinking_config: ThinkingConfig,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub verbose: bool,
    pub custom_system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub session_id: Option<String>,
    pub fallback_models: Vec<String>,
    pub max_retries_per_model: u32,
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
    #[serde(rename = "thinking_chunk")]
    ThinkingChunk {
        content: String,
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
    #[serde(rename = "model_fallback")]
    ModelFallback {
        from_model: String,
        to_model: String,
    },
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

/// Result of a context compaction operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompactResult {
    pub tokens_saved: u64,
    pub summary_tokens: u64,
    pub tokens_before: u64,
    pub tokens_after: u64,
}

/// The core QueryEngine that orchestrates LLM calls, tool execution, and message management.
pub struct QueryEngine {
    config: QueryEngineConfig,
    messages: Vec<Message>,
    pending_messages: Option<Arc<tokio::sync::Mutex<Vec<Message>>>>,
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
            pending_messages: None,
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

    /// Replace the conversation message history (used for session resume).
    pub fn set_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }

    /// Get a reference to the accumulated usage statistics.
    pub fn get_usage(&self) -> &Usage {
        &self.total_usage
    }

    /// Update the thinking configuration at runtime.
    pub fn update_thinking_config(&mut self, config: ThinkingConfig) {
        self.config.thinking_config = config;
    }

    /// Update the model at runtime.
    pub fn update_model(&mut self, model: String) {
        self.config.model = model;
    }

    /// Update the working directory at runtime.
    pub fn update_cwd(&mut self, cwd: std::path::PathBuf) {
        self.config.cwd = cwd;
    }

    /// Get the current model name.
    pub fn get_model(&self) -> &str {
        &self.config.model
    }

    /// Sync messages back from the spawned query loop task.
    /// Must be called after the query loop completes (after draining the event rx).
    pub async fn sync_messages(&mut self) {
        if let Some(pending) = self.pending_messages.take() {
            let msgs = pending.lock().await;
            self.messages = msgs.clone();
        }
    }

    /// Execute context compaction.
    ///
    /// Keeps the most recent `keep_recent` (4) messages and summarises the
    /// older ones via the API, replacing them with a single
    /// `CompactBoundary` system message that contains the summary.
    pub async fn compact(&mut self) -> Result<CompactResult, EngineError> {
        let keep_recent: usize = 4;

        let tokens_before = estimate_tokens(&self.messages);

        if self.messages.len() <= keep_recent {
            return Ok(CompactResult {
                tokens_saved: 0,
                summary_tokens: 0,
                tokens_before,
                tokens_after: tokens_before,
            });
        }

        let split = self.messages.len() - keep_recent;
        let old_messages = &self.messages[..split];
        let recent_messages = self.messages[split..].to_vec();

        // Build a summarisation prompt from the old messages
        let summary_prompt = format!(
            "Summarize the following conversation history concisely, \
             preserving key context, decisions, and file changes:\n\n{}",
            format_messages_for_summary(old_messages)
        );

        // Call the API (non-streaming) to produce a summary
        let summary = self.call_api_for_summary(&summary_prompt).await?;

        let old_token_count = estimate_tokens(old_messages);
        let summary_token_count = estimate_tokens_str(&summary);

        // Build the compact boundary message
        let boundary = Message {
            uuid: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: MessageContent::System {
                subtype: crate::models::message::SystemSubtype::CompactBoundary,
                content: summary,
            },
        };

        // Replace messages: boundary + recent
        self.messages = vec![boundary];
        self.messages.extend(recent_messages);

        let tokens_after = estimate_tokens(&self.messages);

        Ok(CompactResult {
            tokens_saved: old_token_count.saturating_sub(summary_token_count),
            summary_tokens: summary_token_count,
            tokens_before,
            tokens_after,
        })
    }

    /// Call the API to generate a summary of old messages.
    async fn call_api_for_summary(&self, prompt: &str) -> Result<String, EngineError> {
        let request = CreateMessageRequest {
            model: self.config.model.clone(),
            messages: vec![serde_json::json!({
                "role": "user",
                "content": prompt,
            })],
            system: Some(vec![serde_json::json!({
                "type": "text",
                "text": "You are a conversation summariser. Produce a concise summary.",
            })]),
            tools: None,
            max_tokens: 4096,
            stream: true,
            thinking: None,
            metadata: None,
        };

        let stream_result = self.config.api_client.create_message_stream(request).await;
        let mut stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                return Err(EngineError {
                    code: "api_error".to_string(),
                    message: format!("Failed to call API for summary: {}", e),
                    details: None,
                });
            }
        };

        let mut summary_text = String::new();
        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => match event {
                    crate::api::client::ApiStreamEvent::ContentBlockDelta { delta, .. } => {
                        if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                            summary_text.push_str(text);
                        }
                    }
                    crate::api::client::ApiStreamEvent::MessageStop => break,
                    crate::api::client::ApiStreamEvent::Error { error } => {
                        return Err(EngineError {
                            code: error.error_type,
                            message: error.message,
                            details: None,
                        });
                    }
                    _ => {}
                },
                Err(e) => {
                    return Err(EngineError {
                        code: "stream_error".to_string(),
                        message: format!("{}", e),
                        details: None,
                    });
                }
            }
        }

        if summary_text.is_empty() {
            return Err(EngineError {
                code: "empty_summary".to_string(),
                message: "API returned an empty summary".to_string(),
                details: None,
            });
        }

        Ok(summary_text)
    }

    /// Submit a user message and process the response loop.
    /// Returns a receiver that yields EngineEvent items.
    pub async fn submit_message(
        &mut self,
        prompt: String,
    ) -> mpsc::Receiver<EngineEvent> {
        self.submit_message_with_attachments(prompt, None).await
    }

    pub async fn submit_message_with_attachments(
        &mut self,
        prompt: String,
        attachments: Option<Vec<serde_json::Value>>,
    ) -> mpsc::Receiver<EngineEvent> {
        let (tx, rx) = mpsc::channel(256);

        // Build user message content: plain string or multimodal array
        let content = if let Some(att) = attachments {
            if att.is_empty() {
                Value::String(prompt)
            } else {
                let mut blocks: Vec<Value> = Vec::new();
                for a in att {
                    blocks.push(a);
                }
                if !prompt.is_empty() {
                    blocks.push(serde_json::json!({"type": "text", "text": prompt}));
                }
                Value::Array(blocks)
            }
        } else {
            Value::String(prompt)
        };

        // Build the user message and append to history
        let user_msg = Message {
            uuid: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: MessageContent::User {
                message: ApiUserMessage {
                    role: "user".to_string(),
                    content,
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
            project_instructions: load_project_instructions(&self.config.cwd),
            git_info: get_git_info(&self.config.cwd),
            thinking_config: self.config.thinking_config.clone(),
            abort_rx: self.abort_rx.clone(),
            session_id: self.config.session_id.clone(),
            fallback_models: self.config.fallback_models.clone(),
            max_retries_per_model: self.config.max_retries_per_model,
        };

        let messages_shared = Arc::new(tokio::sync::Mutex::new(self.messages.clone()));
        let messages_for_task = Arc::clone(&messages_shared);

        tokio::spawn(async move {
            let mut msgs = messages_for_task.lock().await;
            run_query_loop(&mut msgs, loop_config, tx).await;
        });

        self.pending_messages = Some(messages_shared);

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
pub struct QueryLoopConfig {
    pub api_client: Arc<UnifiedClient>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub model: String,
    pub max_turns: Option<u32>,
    pub cwd: PathBuf,
    pub custom_system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub project_instructions: Option<String>,
    pub git_info: Option<GitInfo>,
    pub thinking_config: ThinkingConfig,
    pub abort_rx: watch::Receiver<bool>,
    pub session_id: Option<String>,
    pub fallback_models: Vec<String>,
    pub max_retries_per_model: u32,
}

impl QueryLoopConfig {
    fn is_aborted(&self) -> bool {
        *self.abort_rx.borrow()
    }
}

/// The core query loop that calls the LLM, processes tool uses, and loops until done.
async fn run_query_loop(
    messages: &mut Vec<Message>,
    config: QueryLoopConfig,
    tx: mpsc::Sender<EngineEvent>,
) {
    let start_time = std::time::Instant::now();
    let mut turn_count = 0u32;
    let mut total_usage = EMPTY_USAGE;
    let mut cost_tracker = CostTracker::new();
    cost_tracker.reset_query();

    // Open transcript writer if session_id is available
    let mut transcript_writer = config.session_id.as_ref().and_then(|sid| {
        TranscriptWriter::open(sid).ok()
    });

    // Helper closure to append a transcript entry (errors are silently ignored)
    fn append_transcript(writer: &mut Option<TranscriptWriter>, entry: &TranscriptEntry) {
        if let Some(w) = writer.as_mut() {
            let _ = w.append(entry);
        }
    }

    // Write the user message that was just added (last message in the vec)
    if let Some(last_msg) = messages.last() {
        append_transcript(&mut transcript_writer, &TranscriptEntry {
            timestamp: last_msg.timestamp.clone(),
            entry_type: TranscriptEntryType::UserMessage,
            data: serde_json::to_value(last_msg).unwrap_or_default(),
        });
    }

    // Build FallbackController from config
    let fallback_config = BaoclawConfig {
        model: config.model.clone(),
        fallback_models: config.fallback_models.clone(),
        max_retries_per_model: config.max_retries_per_model,
        api_type: "anthropic".to_string(),
        openai_base_url: None,
        extra: std::collections::HashMap::new(),
    };
    let mut fallback_controller = FallbackController::new(&fallback_config);

    loop {
        // Check abort
        if config.is_aborted() {
            let _ = tx.send(EngineEvent::Result(QueryResult {
                status: QueryStatus::Aborted,
                text: None,
                stop_reason: None,
                total_cost_usd: cost_tracker.total_cost(),
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
                    total_cost_usd: cost_tracker.total_cost(),
                    usage: total_usage,
                    num_turns: turn_count,
                    duration_ms: start_time.elapsed().as_millis() as u64,
                })).await;
                return;
            }
        }

        // Build API request using the current model from fallback controller
        let current_config = QueryLoopConfig {
            api_client: Arc::clone(&config.api_client),
            tools: config.tools.clone(),
            model: fallback_controller.current_model().to_string(),
            max_turns: config.max_turns,
            cwd: config.cwd.clone(),
            custom_system_prompt: config.custom_system_prompt.clone(),
            append_system_prompt: config.append_system_prompt.clone(),
            project_instructions: config.project_instructions.clone(),
            git_info: config.git_info.clone(),
            thinking_config: config.thinking_config.clone(),
            abort_rx: config.abort_rx.clone(),
            session_id: config.session_id.clone(),
            fallback_models: config.fallback_models.clone(),
            max_retries_per_model: config.max_retries_per_model,
        };
        let request = build_api_request(&messages, &current_config);

        // Call LLM API (streaming) with rate-limit fallback handling
        let stream_result = config.api_client.create_message_stream(request).await;
        let mut stream = match stream_result {
            Ok(s) => s,
            Err(ApiError::RateLimited) => {
                // Handle rate limit with fallback controller
                match fallback_controller.on_rate_limit() {
                    FallbackAction::Retry { model, attempt, delay } => {
                        eprintln!("Rate limited on {}, retrying (attempt {})...", model, attempt);
                        tokio::time::sleep(delay).await;
                        continue; // retry the loop
                    }
                    FallbackAction::Fallback { from, to } => {
                        eprintln!("Rate limited on {}, falling back to {}", from, to);
                        let _ = tx.send(EngineEvent::ModelFallback {
                            from_model: from,
                            to_model: to,
                        }).await;
                        continue; // retry with new model
                    }
                    FallbackAction::Exhausted { models_tried, total_retries } => {
                        let _ = tx.send(EngineEvent::Error(EngineError {
                            code: "all_models_exhausted".to_string(),
                            message: format!(
                                "All models exhausted after {} retries. Tried: {}",
                                total_retries,
                                models_tried.join(", ")
                            ),
                            details: Some(serde_json::json!({
                                "models_tried": models_tried,
                                "total_retries": total_retries,
                            })),
                        })).await;
                        return;
                    }
                }
            }
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
        let mut current_thinking_text = String::new();
        let mut stop_reason: Option<String> = None;
        // Track what kind of block we're in: "text", "tool_use", "thinking", or ""
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
                            "thinking" => {
                                current_thinking_text = String::new();
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
                            "thinking_delta" => {
                                if let Some(text) = delta.get("thinking").and_then(|v| v.as_str()) {
                                    current_thinking_text.push_str(text);
                                    // Emit ThinkingChunk to CLI
                                    let _ = tx.send(EngineEvent::ThinkingChunk {
                                        content: text.to_string(),
                                    }).await;
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
                            "thinking" => {
                                if !current_thinking_text.is_empty() {
                                    assistant_content_blocks.push(ContentBlock::Thinking {
                                        thinking: current_thinking_text.clone(),
                                    });
                                }
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
                        // Accumulate cost from message_delta usage
                        let delta_usage = Usage {
                            input_tokens: usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                            output_tokens: usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                            cache_creation_input_tokens: usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()),
                            cache_read_input_tokens: usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()),
                        };
                        cost_tracker.accumulate(&delta_usage, &config.model);
                    }
                    ApiStreamEvent::MessageStart { message } => {
                        // Extract usage from message_start if present
                        if let Some(usage_val) = message.get("usage") {
                            accumulate_usage(&mut total_usage, usage_val);
                            // Accumulate cost from message_start usage
                            let start_usage = Usage {
                                input_tokens: usage_val.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                                output_tokens: usage_val.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                                cache_creation_input_tokens: usage_val.get("cache_creation_input_tokens").and_then(|v| v.as_u64()),
                                cache_read_input_tokens: usage_val.get("cache_read_input_tokens").and_then(|v| v.as_u64()),
                            };
                            cost_tracker.accumulate(&start_usage, &config.model);
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
                cost_usd: cost_tracker.current_query_cost(),
                duration_ms: 0,
            },
        };
        messages.push(assistant_msg.clone());

        // Write assistant message to transcript
        append_transcript(&mut transcript_writer, &TranscriptEntry {
            timestamp: assistant_msg.timestamp.clone(),
            entry_type: TranscriptEntryType::AssistantMessage,
            data: serde_json::to_value(&assistant_msg).unwrap_or_default(),
        });

        // Push cost data to CLI via StateUpdate
        let _ = tx.send(EngineEvent::StateUpdate {
            patch: serde_json::json!({
                "total_cost_usd": cost_tracker.total_cost(),
                "current_query_cost_usd": cost_tracker.current_query_cost(),
                "usage": {
                    "input_tokens": total_usage.input_tokens,
                    "output_tokens": total_usage.output_tokens,
                    "cache_creation_input_tokens": total_usage.cache_creation_input_tokens,
                    "cache_read_input_tokens": total_usage.cache_read_input_tokens,
                }
            }),
        }).await;

        // Check for tool_use blocks
        let tool_uses = extract_tool_uses(&assistant_content_blocks);

        if tool_uses.is_empty() {
            // Check for context window exceeded — auto-compact and retry
            if stop_reason.as_deref() == Some("model_context_window_exceeded") {
                eprintln!("Context window exceeded, auto-compacting...");
                let _ = tx.send(EngineEvent::AssistantChunk {
                    content: "🗜️ 上下文窗口已满，正在自动压缩对话历史...\n".to_string(),
                    tool_use_id: None,
                }).await;

                // Remove the empty assistant message we just added
                if let Some(last) = messages.last() {
                    if matches!(&last.content, MessageContent::Assistant { .. }) {
                        messages.pop();
                    }
                }
                // Also remove the user message (we'll re-add it after compact)
                let user_msg = messages.pop();

                // Inline compact: keep last 4 messages, summarize the rest
                let keep_recent: usize = 4;
                if messages.len() > keep_recent {
                    let split = messages.len() - keep_recent;
                    let old_messages = &messages[..split];
                    let summary_prompt = format!(
                        "Summarize the following conversation history concisely, \
                         preserving key context, decisions, and file changes:\n\n{}",
                        format_messages_for_summary(old_messages)
                    );
                    // Call API for summary (non-streaming)
                    let summary_request = CreateMessageRequest {
                        model: config.model.clone(),
                        messages: vec![serde_json::json!({
                            "role": "user",
                            "content": summary_prompt,
                        })],
                        system: Some(vec![serde_json::json!({
                            "type": "text",
                            "text": "You are a conversation summariser. Produce a concise summary.",
                        })]),
                        tools: None,
                        max_tokens: 4096,
                        stream: true,
                        thinking: None,
                        metadata: None,
                    };
                    let summary_result = async {
                        let mut stream = config.api_client.create_message_stream(summary_request).await
                            .map_err(|e| format!("{}", e))?;
                        let mut text = String::new();
                        while let Some(event_result) = stream.next().await {
                            match event_result {
                                Ok(ApiStreamEvent::ContentBlockDelta { delta, .. }) => {
                                    if let Some(t) = delta.get("text").and_then(|v| v.as_str()) {
                                        text.push_str(t);
                                    }
                                }
                                Ok(ApiStreamEvent::MessageStop) => break,
                                Ok(ApiStreamEvent::Error { error }) => {
                                    return Err(format!("{}: {}", error.error_type, error.message));
                                }
                                Err(e) => return Err(format!("{}", e)),
                                _ => {}
                            }
                        }
                        Ok::<String, String>(text)
                    }.await;

                    match summary_result {
                        Ok(summary_text) if !summary_text.is_empty() => {

                            let recent = messages[split..].to_vec();
                            messages.clear();
                            messages.push(Message {
                                uuid: uuid::Uuid::new_v4().to_string(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                content: MessageContent::System {
                                    subtype: crate::models::message::SystemSubtype::CompactBoundary,
                                    content: summary_text,
                                },
                            });
                            messages.extend(recent);
                            eprintln!("Auto-compact done, {} messages remaining", messages.len());
                        }
                        Ok(_) | Err(_) => {
                            eprintln!("Auto-compact summary failed, truncating instead");
                            let recent = messages[split..].to_vec();
                            messages.clear();
                            messages.extend(recent);
                        }
                    }
                }

                // Re-add the user message and retry
                if let Some(msg) = user_msg {
                    messages.push(msg);
                }
                let _ = tx.send(EngineEvent::AssistantChunk {
                    content: "✅ 压缩完成，正在重试...\n\n".to_string(),
                    tool_use_id: None,
                }).await;
                continue; // retry the query loop
            }

            // No tools → query complete
            let text = extract_text(&assistant_content_blocks);
            let _ = tx.send(EngineEvent::Result(QueryResult {
                status: QueryStatus::Complete,
                text,
                stop_reason,
                total_cost_usd: cost_tracker.total_cost(),
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

            // Write tool use to transcript
            append_transcript(&mut transcript_writer, &TranscriptEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                entry_type: TranscriptEntryType::ToolUse,
                data: serde_json::json!({
                    "tool_name": tu.name,
                    "input": tu.input,
                    "tool_use_id": tu.id,
                }),
            });
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

            // Write tool result to transcript
            append_transcript(&mut transcript_writer, &TranscriptEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                entry_type: TranscriptEntryType::ToolResult,
                data: serde_json::json!({
                    "tool_use_id": result.tool_use_id,
                    "output": result.output,
                    "is_error": result.is_error,
                }),
            });
        }

        // Build tool result user message and append to messages
        let tool_result_msg = build_tool_result_message(&tool_results);
        messages.push(tool_result_msg);

        turn_count += 1;
    }
}

/// Load project instructions from BAOCLAW.md files.
///
/// Scans `.baoclaw/BAOCLAW.md` first, then `BAOCLAW.md` in the given directory.
/// Returns the content of the first found non-empty file, or None.
pub fn load_project_instructions(cwd: &Path) -> Option<String> {
    let paths = [
        cwd.join(".baoclaw").join("BAOCLAW.md"),
        cwd.join("BAOCLAW.md"),
    ];
    for p in &paths {
        if let Ok(content) = std::fs::read_to_string(p) {
            if !content.trim().is_empty() {
                return Some(content);
            }
        }
    }
    None
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
        thinking: match &config.thinking_config {
            ThinkingConfig::Disabled => None,
            ThinkingConfig::Adaptive => Some(serde_json::json!({
                "type": "enabled",
                "budget_tokens": 10240
            })),
            ThinkingConfig::Enabled { budget_tokens } => Some(serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget_tokens
            })),
        },
        metadata: None,
    }
}

/// Build the system prompt from config.
pub fn build_system_prompt(config: &QueryLoopConfig) -> Option<Vec<Value>> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(custom) = &config.custom_system_prompt {
        parts.push(custom.clone());
    } else {
        parts.push("You are a helpful AI coding assistant.".to_string());
    }

    // Inject project instructions from BAOCLAW.md
    if let Some(instructions) = &config.project_instructions {
        parts.push(format!(
            "# Project Instructions (from BAOCLAW.md)\n\n{}",
            instructions
        ));
    }

    // Inject git repository information
    if let Some(git_info) = &config.git_info {
        let mut git_parts: Vec<String> = Vec::new();
        if let Some(branch) = &git_info.branch {
            git_parts.push(format!("Current git branch: {}", branch));
        }
        if git_info.has_changes {
            let mut change_lines: Vec<String> = Vec::new();
            if !git_info.staged_files.is_empty() {
                change_lines.push(format!("Staged: {}", git_info.staged_files.join(", ")));
            }
            if !git_info.modified_files.is_empty() {
                change_lines.push(format!("Modified: {}", git_info.modified_files.join(", ")));
            }
            if !git_info.untracked_files.is_empty() {
                change_lines.push(format!("Untracked: {}", git_info.untracked_files.join(", ")));
            }
            git_parts.push(format!("Changed files:\n{}", change_lines.join("\n")));
        }
        if !git_parts.is_empty() {
            parts.push(format!("# Git Status\n\n{}", git_parts.join("\n")));
        }
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
        // Strip large base64 image data from tool output to avoid bloating context
        let output = strip_base64_images(&r.output);
        serde_json::json!({
            "type": "tool_result",
            "tool_use_id": r.tool_use_id,
            "content": output,
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

/// Strip large base64 image data from tool output values.
/// Replaces image content with a short placeholder to keep context small.
fn strip_base64_images(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            // Check if this is a JSON string containing MCP image content
            if s.len() > 10_000 {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    let stripped = strip_base64_images(&parsed);
                    return Value::String(serde_json::to_string(&stripped).unwrap_or_else(|_| s.clone()));
                }
                // Check for raw base64 data patterns
                if s.contains("iVBOR") || s.contains("data:image") {
                    return Value::String("[image data removed to save context]".to_string());
                }
            }
            value.clone()
        }
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                if k == "data" {
                    if let Value::String(s) = v {
                        if s.len() > 1000 && (s.starts_with("iVBOR") || s.starts_with("/9j/")) {
                            new_map.insert(k.clone(), Value::String("[image: base64 data removed]".to_string()));
                            continue;
                        }
                    }
                }
                new_map.insert(k.clone(), strip_base64_images(v));
            }
            Value::Object(new_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| strip_base64_images(v)).collect())
        }
        _ => value.clone(),
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

/// Estimate the token count for a slice of messages.
///
/// Uses a simple heuristic: ~4 characters per token.
pub fn estimate_tokens(messages: &[Message]) -> u64 {
    let total_chars: usize = messages
        .iter()
        .map(|m| {
            match &m.content {
                MessageContent::User { message, .. } => {
                    serde_json::to_string(&message.content)
                        .unwrap_or_default()
                        .len()
                }
                MessageContent::Assistant { message, .. } => {
                    message
                        .content
                        .iter()
                        .map(|block| match block {
                            ContentBlock::Text { text } => text.len(),
                            ContentBlock::ToolUse { input, .. } => {
                                serde_json::to_string(input).unwrap_or_default().len()
                            }
                            ContentBlock::Thinking { thinking } => thinking.len(),
                            ContentBlock::Image { source } => source.data.len(),
                            ContentBlock::Document { source } => source.data.len(),
                        })
                        .sum()
                }
                MessageContent::System { content, .. } => content.len(),
                MessageContent::Progress { data, .. } => {
                    serde_json::to_string(data).unwrap_or_default().len()
                }
            }
        })
        .sum();
    (total_chars as u64) / 4
}

/// Estimate the token count for a string.
///
/// Uses a simple heuristic: ~4 characters per token.
pub fn estimate_tokens_str(s: &str) -> u64 {
    (s.len() as u64) / 4
}

/// Format messages into a human-readable string for summarisation.
pub fn format_messages_for_summary(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| match &m.content {
            MessageContent::User { message, .. } => {
                let text = match &message.content {
                    Value::String(s) => s.clone(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                };
                format!("User: {}", text)
            }
            MessageContent::Assistant { message, .. } => {
                let text: String = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("Assistant: {}", text)
            }
            MessageContent::System { content, .. } => {
                format!("System: {}", content)
            }
            MessageContent::Progress { .. } => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::{AnthropicClient, ApiClientConfig};
    use crate::models::message::ContentBlock;
    use serde_json::json;

    fn make_config() -> QueryEngineConfig {
        let api_client = Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
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
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
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
            api_client: Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
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
            project_instructions: None,
            git_info: None,
            thinking_config: ThinkingConfig::Disabled,
            abort_rx,
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
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
            api_client: Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
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
            project_instructions: None,
            git_info: None,
            thinking_config: ThinkingConfig::Disabled,
            abort_rx,
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
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
            api_client: Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
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
            project_instructions: None,
            git_info: None,
            thinking_config: ThinkingConfig::Disabled,
            abort_rx,
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
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

    // --- load_project_instructions tests ---

    #[test]
    fn test_load_project_instructions_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_project_instructions(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_load_project_instructions_baoclaw_dir_file() {
        let dir = tempfile::tempdir().unwrap();
        let baoclaw_dir = dir.path().join(".baoclaw");
        std::fs::create_dir_all(&baoclaw_dir).unwrap();
        std::fs::write(baoclaw_dir.join("BAOCLAW.md"), "Use Rust conventions").unwrap();
        let result = load_project_instructions(dir.path());
        assert_eq!(result, Some("Use Rust conventions".to_string()));
    }

    #[test]
    fn test_load_project_instructions_root_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("BAOCLAW.md"), "Root instructions").unwrap();
        let result = load_project_instructions(dir.path());
        assert_eq!(result, Some("Root instructions".to_string()));
    }

    #[test]
    fn test_load_project_instructions_priority() {
        // .baoclaw/BAOCLAW.md takes priority over BAOCLAW.md
        let dir = tempfile::tempdir().unwrap();
        let baoclaw_dir = dir.path().join(".baoclaw");
        std::fs::create_dir_all(&baoclaw_dir).unwrap();
        std::fs::write(baoclaw_dir.join("BAOCLAW.md"), "Priority content").unwrap();
        std::fs::write(dir.path().join("BAOCLAW.md"), "Fallback content").unwrap();
        let result = load_project_instructions(dir.path());
        assert_eq!(result, Some("Priority content".to_string()));
    }

    #[test]
    fn test_load_project_instructions_empty_file_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let baoclaw_dir = dir.path().join(".baoclaw");
        std::fs::create_dir_all(&baoclaw_dir).unwrap();
        // Empty file in .baoclaw/ should be skipped
        std::fs::write(baoclaw_dir.join("BAOCLAW.md"), "").unwrap();
        std::fs::write(dir.path().join("BAOCLAW.md"), "Fallback content").unwrap();
        let result = load_project_instructions(dir.path());
        assert_eq!(result, Some("Fallback content".to_string()));
    }

    #[test]
    fn test_load_project_instructions_whitespace_only_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("BAOCLAW.md"), "   \n  \t  ").unwrap();
        let result = load_project_instructions(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_load_project_instructions_both_empty() {
        let dir = tempfile::tempdir().unwrap();
        let baoclaw_dir = dir.path().join(".baoclaw");
        std::fs::create_dir_all(&baoclaw_dir).unwrap();
        std::fs::write(baoclaw_dir.join("BAOCLAW.md"), "").unwrap();
        std::fs::write(dir.path().join("BAOCLAW.md"), "  ").unwrap();
        let result = load_project_instructions(dir.path());
        assert!(result.is_none());
    }

    // --- build_system_prompt with project_instructions tests ---

    #[test]
    fn test_build_system_prompt_with_project_instructions() {
        let (_abort_tx, abort_rx) = watch::channel(false);
        let config = QueryLoopConfig {
            api_client: Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
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
            project_instructions: Some("Always use snake_case".to_string()),
            git_info: None,
            thinking_config: ThinkingConfig::Disabled,
            abort_rx,
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
        };
        let system = build_system_prompt(&config);
        assert!(system.is_some());
        let text = system.unwrap()[0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("# Project Instructions (from BAOCLAW.md)"));
        assert!(text.contains("Always use snake_case"));
    }

    #[test]
    fn test_build_system_prompt_no_project_instructions() {
        let (_abort_tx, abort_rx) = watch::channel(false);
        let config = QueryLoopConfig {
            api_client: Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
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
            project_instructions: None,
            git_info: None,
            thinking_config: ThinkingConfig::Disabled,
            abort_rx,
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
        };
        let system = build_system_prompt(&config);
        assert!(system.is_some());
        let text = system.unwrap()[0]["text"].as_str().unwrap().to_string();
        assert!(!text.contains("Project Instructions"));
    }

    // --- Compact helper function tests ---

    /// Helper to create a simple user message for testing.
    fn make_user_msg(text: &str) -> Message {
        Message {
            uuid: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: MessageContent::User {
                message: ApiUserMessage {
                    role: "user".to_string(),
                    content: Value::String(text.to_string()),
                },
                is_meta: false,
                tool_use_result: None,
            },
        }
    }

    /// Helper to create a simple assistant message for testing.
    fn make_assistant_msg(text: &str) -> Message {
        Message {
            uuid: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: text.to_string(),
                    }],
                    stop_reason: Some("end_turn".to_string()),
                    usage: None,
                },
                cost_usd: 0.0,
                duration_ms: 0,
            },
        }
    }

    #[tokio::test]
    async fn test_compact_too_few_messages_no_compression() {
        // With <= 4 messages, compact should return tokens_saved=0
        let mut engine = QueryEngine::new(make_config());
        engine.set_messages(vec![
            make_user_msg("hello"),
            make_assistant_msg("hi"),
        ]);
        let result = engine.compact().await.unwrap();
        assert_eq!(result.tokens_saved, 0);
        assert_eq!(result.summary_tokens, 0);
        // Messages should be unchanged
        assert_eq!(engine.get_messages().len(), 2);
    }

    #[tokio::test]
    async fn test_compact_exactly_four_messages_no_compression() {
        let mut engine = QueryEngine::new(make_config());
        engine.set_messages(vec![
            make_user_msg("msg1"),
            make_assistant_msg("msg2"),
            make_user_msg("msg3"),
            make_assistant_msg("msg4"),
        ]);
        let result = engine.compact().await.unwrap();
        assert_eq!(result.tokens_saved, 0);
        assert_eq!(result.summary_tokens, 0);
        assert_eq!(engine.get_messages().len(), 4);
    }

    #[tokio::test]
    async fn test_compact_zero_messages_no_compression() {
        let mut engine = QueryEngine::new(make_config());
        let result = engine.compact().await.unwrap();
        assert_eq!(result.tokens_saved, 0);
        assert_eq!(result.summary_tokens, 0);
        assert_eq!(engine.get_messages().len(), 0);
    }

    #[test]
    fn test_estimate_tokens_empty() {
        let messages: Vec<Message> = vec![];
        assert_eq!(estimate_tokens(&messages), 0);
    }

    #[test]
    fn test_estimate_tokens_user_message() {
        // "hello world" = 11 chars → 11/4 = 2 tokens (integer division)
        let messages = vec![make_user_msg("hello world")];
        let tokens = estimate_tokens(&messages);
        // The serialized form includes quotes: "\"hello world\"" = 13 chars → 3 tokens
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_tokens_str_basic() {
        assert_eq!(estimate_tokens_str(""), 0);
        assert_eq!(estimate_tokens_str("abcd"), 1);
        assert_eq!(estimate_tokens_str("abcdefgh"), 2);
    }

    #[test]
    fn test_format_messages_for_summary_empty() {
        let messages: Vec<Message> = vec![];
        let result = format_messages_for_summary(&messages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_messages_for_summary_user_and_assistant() {
        let messages = vec![
            make_user_msg("What is Rust?"),
            make_assistant_msg("Rust is a systems programming language."),
        ];
        let result = format_messages_for_summary(&messages);
        assert!(result.contains("User: What is Rust?"));
        assert!(result.contains("Assistant: Rust is a systems programming language."));
    }

    #[test]
    fn test_format_messages_for_summary_system_message() {
        let messages = vec![Message {
            uuid: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: MessageContent::System {
                subtype: crate::models::message::SystemSubtype::LocalCommand,
                content: "System event occurred".to_string(),
            },
        }];
        let result = format_messages_for_summary(&messages);
        assert!(result.contains("System: System event occurred"));
    }

    #[test]
    fn test_compact_result_serialization() {
        let result = CompactResult {
            tokens_saved: 1500,
            summary_tokens: 200,
            tokens_before: 2000,
            tokens_after: 500,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["tokens_saved"], 1500);
        assert_eq!(json["summary_tokens"], 200);
        assert_eq!(json["tokens_before"], 2000);
        assert_eq!(json["tokens_after"], 500);
    }

    #[test]
    fn test_compact_result_deserialization() {
        let json = json!({"tokens_saved": 3000, "summary_tokens": 500, "tokens_before": 4000, "tokens_after": 1000});
        let result: CompactResult = serde_json::from_value(json).unwrap();
        assert_eq!(result.tokens_saved, 3000);
        assert_eq!(result.summary_tokens, 500);
        assert_eq!(result.tokens_before, 4000);
        assert_eq!(result.tokens_after, 1000);
    }

    // --- Thinking config in build_api_request tests ---

    fn make_loop_config_with_thinking(thinking_config: ThinkingConfig) -> QueryLoopConfig {
        let (_abort_tx, abort_rx) = watch::channel(false);
        QueryLoopConfig {
            api_client: Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
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
            project_instructions: None,
            git_info: None,
            thinking_config,
            abort_rx,
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
        }
    }

    fn make_test_messages() -> Vec<Message> {
        vec![Message {
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
        }]
    }

    #[test]
    fn test_build_api_request_thinking_disabled() {
        let config = make_loop_config_with_thinking(ThinkingConfig::Disabled);
        let messages = make_test_messages();
        let request = build_api_request(&messages, &config);
        assert!(request.thinking.is_none(), "Thinking should be None when disabled");
    }

    #[test]
    fn test_build_api_request_thinking_adaptive() {
        let config = make_loop_config_with_thinking(ThinkingConfig::Adaptive);
        let messages = make_test_messages();
        let request = build_api_request(&messages, &config);
        assert!(request.thinking.is_some(), "Thinking should be Some when adaptive");
        let thinking = request.thinking.unwrap();
        assert_eq!(thinking["type"], "enabled");
        assert_eq!(thinking["budget_tokens"], 10240);
    }

    #[test]
    fn test_build_api_request_thinking_enabled_default_budget() {
        let config = make_loop_config_with_thinking(ThinkingConfig::Enabled { budget_tokens: 10240 });
        let messages = make_test_messages();
        let request = build_api_request(&messages, &config);
        assert!(request.thinking.is_some(), "Thinking should be Some when enabled");
        let thinking = request.thinking.unwrap();
        assert_eq!(thinking["type"], "enabled");
        assert_eq!(thinking["budget_tokens"], 10240);
    }

    #[test]
    fn test_build_api_request_thinking_enabled_custom_budget() {
        let config = make_loop_config_with_thinking(ThinkingConfig::Enabled { budget_tokens: 32768 });
        let messages = make_test_messages();
        let request = build_api_request(&messages, &config);
        assert!(request.thinking.is_some(), "Thinking should be Some when enabled");
        let thinking = request.thinking.unwrap();
        assert_eq!(thinking["type"], "enabled");
        assert_eq!(thinking["budget_tokens"], 32768);
    }

    #[test]
    fn test_build_api_request_thinking_enabled_serialization() {
        // Verify the full request serializes correctly with thinking
        let config = make_loop_config_with_thinking(ThinkingConfig::Enabled { budget_tokens: 16384 });
        let messages = make_test_messages();
        let request = build_api_request(&messages, &config);
        let json = serde_json::to_value(&request).unwrap();
        assert!(json.get("thinking").is_some(), "Serialized request should contain thinking field");
        assert_eq!(json["thinking"]["type"], "enabled");
        assert_eq!(json["thinking"]["budget_tokens"], 16384);
    }

    #[test]
    fn test_build_api_request_thinking_disabled_serialization() {
        // Verify the full request serializes correctly without thinking
        let config = make_loop_config_with_thinking(ThinkingConfig::Disabled);
        let messages = make_test_messages();
        let request = build_api_request(&messages, &config);
        let json = serde_json::to_value(&request).unwrap();
        assert!(json.get("thinking").is_none(), "Serialized request should not contain thinking field when disabled");
    }

    #[test]
    fn test_update_thinking_config() {
        let mut engine = QueryEngine::new(make_config());
        // Default is Disabled
        engine.update_thinking_config(ThinkingConfig::Enabled { budget_tokens: 8192 });
        // Verify by checking the config was updated (we can't directly access config,
        // but we can verify through the ThinkingConfig serialization)
        engine.update_thinking_config(ThinkingConfig::Disabled);
        engine.update_thinking_config(ThinkingConfig::Adaptive);
        // No panic means success
    }

    #[test]
    fn test_thinking_chunk_event_serialization() {
        let event = EngineEvent::ThinkingChunk {
            content: "Let me analyze this...".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "thinking_chunk");
        assert_eq!(json["content"], "Let me analyze this...");
    }

    #[test]
    fn test_thinking_chunk_event_roundtrip() {
        let event = EngineEvent::ThinkingChunk {
            content: "Step 1: Parse the input".to_string(),
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let deserialized: EngineEvent = serde_json::from_str(&json_str).unwrap();
        match deserialized {
            EngineEvent::ThinkingChunk { content } => {
                assert_eq!(content, "Step 1: Parse the input");
            }
            _ => panic!("Expected ThinkingChunk"),
        }
    }
}
