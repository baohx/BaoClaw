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
use crate::engine::git_info::{get_git_info, get_git_info_async, GitInfo};
use crate::engine::session_memory::SessionMemory;
use crate::engine::token_counter::BudgetStatus;
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
    /// Model context window (tokens). Default: 200_000 (Claude).
    pub context_window: u64,
    /// Auto-compact threshold as fraction of `context_window`. Default: 0.7.
    pub auto_compact_threshold_ratio: f64,
    /// For sub-agents: the turn_id of the parent agent's current turn.
    pub parent_turn_id: Option<u32>,
    /// For sub-agents: a short label describing the task (shown in CLI).
    pub agent_label: Option<String>,
    /// Session memory for rolling summaries (optional — only created when session_id is set).
    pub session_memory: Option<Arc<SessionMemory>>,
    /// Shared file cache for reducing redundant file reads.
    pub file_cache: Option<Arc<tokio::sync::Mutex<crate::engine::file_cache::FileCache>>>,
    /// Tool result store for persisting large outputs to disk.
    pub tool_result_store: Option<Arc<crate::engine::tool_result_store::ToolResultStore>>,
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
    /// Emitted at the start of each LLM turn (one API call + tool loop).
    #[serde(rename = "turn_start")]
    TurnStart {
        turn_id: u32,
        parent_turn_id: Option<u32>,
        agent_label: Option<String>,
    },
    /// Emitted when a turn completes (after all tool calls for that turn).
    #[serde(rename = "turn_end")]
    TurnEnd {
        turn_id: u32,
        duration_ms: u64,
        tool_count: u32,
        input_tokens: u64,
        output_tokens: u64,
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

/// Tracks compact history to adaptively tune the keep_recent parameter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveCompactTracker {
    /// History of compact results for feedback analysis.
    pub history: Vec<CompactFeedback>,
    /// Current adaptive keep_recent value (messages, not turns).
    pub keep_recent: usize,
    /// Running average compression ratio.
    pub avg_compression_ratio: f64,
    /// Running average information loss score (0.0 = no loss, 1.0 = severe).
    pub avg_loss_score: f64,
    /// Number of compacts performed.
    pub compact_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompactFeedback {
    /// Compression ratio: tokens_saved / tokens_before.
    pub compression_ratio: f64,
    /// Tokens before compact.
    pub tokens_before: u64,
    /// Tokens after compact.
    pub tokens_after: u64,
    /// Whether the user re-asked about pre-compact content within next 3 turns.
    pub user_repeated_topic: bool,
    /// Timestamp.
    pub timestamp: String,
}

impl AdaptiveCompactTracker {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            keep_recent: 10, // start with default
            avg_compression_ratio: 0.0,
            avg_loss_score: 0.0,
            compact_count: 0,
        }
    }

    /// Record a compact result and adjust keep_recent for next time.
    pub fn record_compact(&mut self, result: &CompactResult, user_repeated: bool) {
        let ratio = if result.tokens_before > 0 {
            result.tokens_saved as f64 / result.tokens_before as f64
        } else {
            0.0
        };

        self.history.push(CompactFeedback {
            compression_ratio: ratio,
            tokens_before: result.tokens_before,
            tokens_after: result.tokens_after,
            user_repeated_topic: user_repeated,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });

        // Keep only last 50 records
        if self.history.len() > 50 {
            self.history.drain(0..self.history.len() - 50);
        }

        self.compact_count += 1;

        // Update running averages
        self.avg_compression_ratio = self.history.iter()
            .map(|h| h.compression_ratio)
            .sum::<f64>() / self.history.len() as f64;

        let loss_entries: Vec<f64> = self.history.iter()
            .map(|h| if h.user_repeated_topic { 0.3 } else { 0.0 })
            .collect();
        self.avg_loss_score = loss_entries.iter().sum::<f64>() / loss_entries.len() as f64;

        // Adaptive adjustment logic:
        // If loss is high (>0.15), increase keep_recent to preserve more context
        // If compression is poor (<0.3) and loss is low, decrease keep_recent to compact more aggressively
        if self.avg_loss_score > 0.15 {
            // Too much information loss — keep more messages
            self.keep_recent = (self.keep_recent + 4).min(30);
        } else if self.avg_compression_ratio < 0.3 && self.avg_loss_score < 0.05 {
            // Poor compression, low loss — compact more aggressively
            self.keep_recent = if self.keep_recent > 6 { self.keep_recent - 2 } else { 6 };
        } else if self.avg_loss_score < 0.05 && self.avg_compression_ratio > 0.6 {
            // Good compression, low loss — current setting works well
            // Slight decrease to save more tokens
            self.keep_recent = if self.keep_recent > 8 { self.keep_recent - 1 } else { 8 };
        }
        // else: moderate performance, keep current setting
    }

    /// Get the recommended keep_recent value.
    pub fn recommended_keep_recent(&self) -> usize {
        self.keep_recent
    }
}

/// A cached rule file from `.baoclaw/rules/*.md`.
#[derive(Clone, Debug)]
pub struct CachedRule {
    pub filename: String,
    pub content: String,
    pub paths_pattern: Option<String>,
}

/// The core QueryEngine that orchestrates LLM calls, tool execution, and message management.
pub struct QueryEngine {
    config: QueryEngineConfig,
    messages: Vec<Message>,
    pending_messages: Option<Arc<tokio::sync::Mutex<Vec<Message>>>>,
    abort_tx: watch::Sender<bool>,
    abort_rx: watch::Receiver<bool>,
    total_usage: Usage,
    token_counter: Arc<tokio::sync::Mutex<crate::engine::token_counter::TokenCounter>>,
    /// Consecutive compact failures — triggers circuit breaker at MAX_COMPACT_FAILURES.
    compact_fail_count: usize,
    /// Cached project instructions (loaded once in new()).
    cached_project_instructions: Option<String>,
    /// Cached rule files (loaded once in new()).
    cached_rules_raw: Vec<CachedRule>,
    /// Cached git info (loaded once in new(), refreshed async each turn).
    cached_git_info: Option<GitInfo>,
}

impl QueryEngine {
    /// Create a new QueryEngine with the given configuration.
    pub fn new(config: QueryEngineConfig) -> Self {
        let (abort_tx, abort_rx) = watch::channel(false);
        let token_counter = Arc::new(tokio::sync::Mutex::new(
            crate::engine::token_counter::TokenCounter::new(
                config.context_window,
                config.auto_compact_threshold_ratio,
            ),
        ));
        // Pre-load caches (once, avoid re-reading every turn)
        let cached_project_instructions = load_project_instructions(&config.cwd);
        let cached_rules_raw = load_all_rule_files(&config.cwd);
        let cached_git_info = get_git_info(&config.cwd);
        Self {
            config,
            messages: Vec::new(),
            pending_messages: None,
            abort_tx,
            abort_rx,
            total_usage: EMPTY_USAGE,
            token_counter,
            compact_fail_count: 0,
            cached_project_instructions,
            cached_rules_raw,
            cached_git_info,
        }
    }

    /// Signal the engine to abort the current operation.
    pub fn abort(&self) {
        let _ = self.abort_tx.send(true);
    }

    /// Manually refresh the cached project instructions and rules.
    pub fn refresh_context_cache(&mut self) {
        self.cached_project_instructions = load_project_instructions(&self.config.cwd);
        self.cached_rules_raw = load_all_rule_files(&self.config.cwd);
        self.cached_git_info = get_git_info(&self.config.cwd);
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

    /// Load and apply a persisted token baseline for fast startup.
    pub async fn load_token_baseline(&self, session_id: &str) {
        if let Some(baseline) = crate::engine::token_counter::TokenCounter::load_baseline(session_id) {
            let mut counter = self.token_counter.lock().await;
            counter.apply_baseline(baseline);
        }
    }

    /// Seed the new session's memory with a summary from a previous session.
    pub fn seed_session_memory(&self, summary: &str) {
        if let Some(ref sm) = self.config.session_memory {
            if !summary.is_empty() {
                sm.update(summary.to_string());
            }
        }
    }

    /// Get a reference to the session memory (if configured).
    pub fn get_session_memory(&self) -> &Option<Arc<SessionMemory>> {
        &self.config.session_memory
    }

    /// Expose the token counter Arc for external use.
    pub fn token_counter_arc(&self) -> Arc<tokio::sync::Mutex<crate::engine::token_counter::TokenCounter>> {
        Arc::clone(&self.token_counter)
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

    /// Update the session ID at runtime (used when switching projects).
    pub fn update_session_id(&mut self, session_id: String) {
        self.config.session_id = Some(session_id);
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
        // Clean up incomplete tool calls at the end of message history.
        // After abort, the last assistant message may contain tool_use blocks
        // without a corresponding tool_result user message, which causes API errors.
        self.cleanup_incomplete_tool_calls();
    }

    /// Remove trailing assistant messages that have tool_use blocks without
    /// a following tool_result user message.
    /// Clean up message history to ensure it's in a valid state for the next API call.
    /// Fixes: consecutive user messages, trailing tool_use without tool_result, etc.
    fn cleanup_incomplete_tool_calls(&mut self) {
        if self.messages.is_empty() { return; }

        // ---- Pass 1: middle-of-history orphan tool_use repair ----
        // Scan every assistant message; for each tool_use id, the NEXT user
        // message must contain a tool_result with the same id. Any missing id
        // gets a stub tool_result inserted so the API call won't 400.
        let mut i = 0usize;
        while i < self.messages.len() {
            // Collect tool_use ids from this assistant message (if any)
            let tool_use_ids: Vec<String> = match &self.messages[i].content {
                MessageContent::Assistant { message, .. } => message.content.iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };

            if !tool_use_ids.is_empty() {
                // The next message must be a user message with matching tool_results.
                let next_idx = i + 1;
                let next_is_user_with_results = matches!(
                    self.messages.get(next_idx).map(|m| &m.content),
                    Some(MessageContent::User { .. })
                );

                if next_is_user_with_results {
                    let present: std::collections::HashSet<String> = match &self.messages[next_idx].content {
                        MessageContent::User { message, .. } =>
                            extract_tool_result_ids(message).into_iter().collect(),
                        _ => std::collections::HashSet::new(),
                    };
                    let missing: Vec<String> = tool_use_ids.iter()
                        .filter(|id| !present.contains(*id))
                        .cloned()
                        .collect();
                    if !missing.is_empty() {
                        eprintln!("Cleanup: injecting stub tool_results for {} missing id(s) at msg[{}]",
                                  missing.len(), next_idx);
                        if let MessageContent::User { message, .. } = &mut self.messages[next_idx].content {
                            for id in missing {
                                if let Value::Array(ref mut arr) = &mut message.content {
                                    arr.push(serde_json::json!({
                                        "type": "tool_result",
                                        "tool_use_id": id,
                                        "content": "[Tool execution interrupted — result missing]",
                                        "is_error": true,
                                    }));
                                }
                            }
                        }
                    }
                } else {
                    // No user message after assistant-with-tool_use → drop this assistant
                    // (and let the trailing pass below clean up duplicates).
                    eprintln!("Cleanup: dropping mid-history assistant with no following user msg at idx {}", i);
                    self.messages.remove(i);
                    continue; // re-check same index
                }
            }
            i += 1;
        }

        // ---- Pass 2: original trailing cleanup (preserved) ----
        loop {
            if self.messages.is_empty() { break; }
            let last = &self.messages[self.messages.len() - 1];
            match &last.content {
                MessageContent::Assistant { message, .. } => {
                    let has_tool_use = message.content.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));
                    if has_tool_use {
                        eprintln!("Cleanup: removing trailing incomplete tool_use assistant message");
                        self.messages.pop();
                        continue;
                    }
                    break;
                }
                MessageContent::User { .. } => {
                    if self.messages.len() >= 2 {
                        let prev = &self.messages[self.messages.len() - 2];
                        let prev_is_user = matches!(&prev.content, MessageContent::User { .. });
                        if prev_is_user {
                            eprintln!("Cleanup: removing duplicate consecutive user message");
                            self.messages.pop();
                            continue;
                        }
                    }
                    break;
                }
                _ => break,
            }
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

        let mut split = self.messages.len() - keep_recent;

        // Ensure we don't split between tool calls and their results.
        // If old_messages ends with an assistant message containing tool_use,
        // we need to either:
        // 1. Include ALL following tool_result messages in old_messages, OR
        // 2. Move the assistant message to recent_messages (if results are incomplete)
        // This handles cases where one assistant message has multiple tool_use blocks.
        if split > 0 && split < self.messages.len() {
            if let MessageContent::Assistant { message, .. } = &self.messages[split - 1].content {
                // Extract all tool_use IDs from the assistant message
                let tool_use_ids: Vec<&str> = message.content.iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                        _ => None,
                    })
                    .collect();

                if !tool_use_ids.is_empty() {
                    // Scan forward to find all corresponding tool_result messages
                    let mut found_results: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let mut next_idx = split;

                    while next_idx < self.messages.len() {
                        if let MessageContent::User { message, .. } = &self.messages[next_idx].content {
                            let result_ids = extract_tool_result_ids(message);
                            for id in result_ids {
                                if tool_use_ids.contains(&id.as_str()) {
                                    found_results.insert(id);
                                }
                            }
                            // Stop if we've found all tool_use results
                            if found_results.len() == tool_use_ids.len() {
                                break;
                            }
                        }
                        next_idx += 1;
                    }

                    // If we found all tool_result messages, include them in old_messages
                    // Otherwise, move the assistant message to recent_messages to avoid orphaning tool_results
                    if found_results.len() == tool_use_ids.len() {
                        split = next_idx + 1;
                    } else {
                        // Not all results found - move assistant message to recent_messages
                        // This ensures tool_use blocks stay with their tool_result blocks
                        // Safety: ensure split doesn't go below 1
                        if split > 1 {
                            split -= 1;
                        }
                    }
                }
            }
        }

        let old_messages = &self.messages[..split];
        let recent_messages = self.messages[split..].to_vec();

        // Build a summarisation prompt from the old messages (truncate to avoid exceeding API limits)
        let raw_summary = format_messages_for_summary(old_messages);
        let max_summary_chars: usize = 60_000; // ~15k tokens, safe for most APIs
        let truncated_summary = if raw_summary.len() > max_summary_chars {
            format!("{}...\n\n[Conversation truncated, {} total chars]",
                &raw_summary.chars().take(max_summary_chars).collect::<String>(), raw_summary.len())
        } else {
            raw_summary
        };
        let summary_prompt = format!(
            "Summarize the following conversation history concisely, \
             preserving key context, decisions, and file changes:\n\n{}",
            truncated_summary
        );

        // Call the API (non-streaming) to produce a summary.
        // If the API call fails (e.g. 500, context too large), fall back to simple truncation.
        let summary = match self.call_api_for_summary(&summary_prompt).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Compact: summary API failed ({}), falling back to truncation", e.message);
                // Fallback: just use a brief note instead of a real summary
                format!("[Previous conversation ({} messages) was truncated due to context limits]", old_messages.len())
            }
        };

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
        loop {
            let event_result = tokio::select! {
                r = stream.next() => r,
                _ = crate::engine::wait_for_abort(self.abort_rx.clone()) => {
                    eprintln!("Aborted during summary streaming");
                    break;
                }
            };
            let Some(event_result) = event_result else { break; };
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
            eprintln!("Compact: API returned empty summary (model: {})", self.config.model);
            return Err(EngineError {
                code: "empty_summary".to_string(),
                message: "API returned an empty summary. Try /compact again or reduce conversation size.".to_string(),
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
        // Reset abort flag for the new query
        let _ = self.abort_tx.send(false);

        let (tx, rx) = mpsc::channel(256);

        let _ = tx.send(EngineEvent::Progress {
            tool_use_id: String::new(),
            data: serde_json::json!({"message": "Cleaning up previous state..."}),
        }).await;

        // Clean up any mess from previous errors/aborts before adding new message
        self.cleanup_incomplete_tool_calls();

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

        // ── Token budget check: auto-compact if context is too large ──
        let _ = tx.send(EngineEvent::Progress {
            tool_use_id: String::new(),
            data: serde_json::json!({"message": "Estimating token usage..."}),
        }).await;
        // Threshold and context window come from BaoclawConfig (default 70% of 200K).
        // The TokenCounter uses tiktoken + API-calibrated baselines for accuracy.
        // Pre-compute both should_compact and budget_status in a single lock+estimate pass.
        let (should_compact, initial_budget) = {
            let counter = self.token_counter.lock().await;
            let est = counter.current_estimate(&self.messages);
            let should = counter.should_compact_given(est) && self.messages.len() > 5;
            let budget = counter.budget_status_given(est);
            (should, (budget, est))
        };
        if should_compact {
            // Pre-query compact: only allowed for reasonably-sized message lists.
            // If we just resumed with thousands of messages (Tier-3 fallback),
            // compacting here would trigger a 10-min API call — use session_memory
            // instead (no API call needed).
            let msg_count = self.messages.len();
            if msg_count <= 500 {
                eprintln!("Pre-query auto-compact ({} messages, {} tokens)", msg_count, initial_budget.1);
                match self.compact().await {
                    Ok(result) => {
                        eprintln!("Auto-compact: {} -> {} tokens (saved {})",
                            result.tokens_before, result.tokens_after, result.tokens_saved);
                        self.compact_fail_count = 0;
                    }
                    Err(e) => {
                        eprintln!("Auto-compact failed: {}, continuing anyway", e.message);
                        self.compact_fail_count += 1;
                    }
                }
            } else {
                // Too many messages — session resume must have loaded too much.
                // Do a quick session_memory_compact instead (no API call).
                eprintln!("Pre-query: {} messages is too many for API compact, trying session_memory_compact", msg_count);
                if let Some(ref sm) = self.config.session_memory {
                    if sm.is_available() {
                        let mut msgs = self.messages.to_vec();
                        if session_memory_compact(&mut msgs, &sm.get()) {
                            self.messages = msgs;
                            eprintln!("Session-memory compact applied ({} messages remaining)", self.messages.len());
                        }
                    } else {
                        // Last resort: just keep last 100 messages
                        let tail: Vec<_> = self.messages[self.messages.len().saturating_sub(100)..].to_vec();
                        eprintln!("Emergency tail-trim: {} → {} messages", msg_count, tail.len());
                        self.messages = tail;
                    }
                } else {
                    // No session_memory at all — keep last 100
                    let tail: Vec<_> = self.messages[self.messages.len().saturating_sub(100)..].to_vec();
                    eprintln!("Emergency tail-trim (no session_memory): {} → {} messages", msg_count, tail.len());
                    self.messages = tail;
                }
            }
        }

        // Build the config for the spawned loop
        let loop_config = QueryLoopConfig {
            api_client: Arc::clone(&self.config.api_client),
            tools: self.config.tools.clone(),
            model: self.config.model.clone(),
            max_turns: self.config.max_turns,
            cwd: self.config.cwd.clone(),
            custom_system_prompt: self.config.custom_system_prompt.clone(),
            append_system_prompt: self.config.append_system_prompt.clone(),
            project_instructions: self.cached_project_instructions.clone(),
            git_info: self.cached_git_info.clone(),
            thinking_config: self.config.thinking_config.clone(),
            abort_rx: self.abort_rx.clone(),
            session_id: self.config.session_id.clone(),
            fallback_models: self.config.fallback_models.clone(),
            max_retries_per_model: self.config.max_retries_per_model,
            token_counter: Arc::clone(&self.token_counter),
            parent_turn_id: self.config.parent_turn_id,
            agent_label: self.config.agent_label.clone(),
            session_memory: self.config.session_memory.as_ref().map(Arc::clone),
            compact_fail_count: self.compact_fail_count,
            recent_messages_for_rules: self.messages.clone(),
            file_cache: self.config.file_cache.as_ref().map(Arc::clone),
            tool_result_store: self.config.tool_result_store.as_ref().map(Arc::clone),
            initial_budget: Some(initial_budget),
            cached_rules_raw: self.cached_rules_raw.clone(),
            frozen_system_prompt: None,
            frozen_tools: None,
            frozen_hash: None,
            adaptive_compact: AdaptiveCompactTracker::new(),
            tool_health: crate::engine::tool_health::ToolHealthTracker::new(),
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
    /// Tracks input-token usage for auto-compaction decisions.
    pub token_counter: Arc<tokio::sync::Mutex<crate::engine::token_counter::TokenCounter>>,
    /// For sub-agents: the turn_id of the parent agent's current turn.
    pub parent_turn_id: Option<u32>,
    /// For sub-agents: a short label describing the task (shown in CLI).
    pub agent_label: Option<String>,
    /// Session memory (cloned Arc for the spawned task).
    pub session_memory: Option<Arc<SessionMemory>>,
    /// Consecutive compact failures — shared with the engine for circuit breaker.
    pub compact_fail_count: usize,
    /// Recent messages snapshot for rules path-matching (refreshed each turn).
    pub recent_messages_for_rules: Vec<Message>,
    /// Shared file cache for reducing redundant file reads.
    pub file_cache: Option<Arc<tokio::sync::Mutex<crate::engine::file_cache::FileCache>>>,
    /// Tool result store for persisting large outputs to disk.
    pub tool_result_store: Option<Arc<crate::engine::tool_result_store::ToolResultStore>>,
    /// Pre-computed budget status from submit_message_with_attachments (first turn only).
    pub initial_budget: Option<(BudgetStatus, u64)>,
    /// Cached rule files (loaded once, filtered in-memory per turn).
    pub cached_rules_raw: Vec<CachedRule>,
    /// Frozen snapshot of the static system prompt (cached on first build, never changes).
    pub frozen_system_prompt: Option<Vec<Value>>,
    /// Frozen snapshot of the tools list (cached on first build, never changes).
    pub frozen_tools: Option<Vec<Value>>,
    /// Hash of the frozen content for cache invalidation diagnostics.
    pub frozen_hash: Option<u64>,
    /// Adaptive compact tracker — learns optimal keep_recent from history.
    pub adaptive_compact: AdaptiveCompactTracker,
    /// Tool health tracker — learns success/failure rates.
    pub tool_health: crate::engine::tool_health::ToolHealthTracker,
}

impl QueryLoopConfig {
    fn is_aborted(&self) -> bool {
        *self.abort_rx.borrow()
    }
}

/// The core query loop that calls the LLM, processes tool uses, and loops until done.
async fn run_query_loop(
    messages: &mut Vec<Message>,
    mut config: QueryLoopConfig,
    tx: mpsc::Sender<EngineEvent>,
) {
    let start_time = std::time::Instant::now();
    let mut turn_count = 0u32;
    let mut total_usage = EMPTY_USAGE;
    let mut cost_tracker = CostTracker::new();
    cost_tracker.reset_query();

    // Iteration budget pressure tracking (Hermes-style 70/90/100 gradient)
    let mut budget_warned_70: bool = false;
    let mut budget_warned_90: bool = false;

    // Per-turn tracking for TurnStart/TurnEnd events
    let mut turn_id_counter: u32 = 0;
    let mut turn_start_time = std::time::Instant::now();
    let mut turn_tool_count: u32 = 0;
    let mut turn_input_tokens_at_start: u64 = 0;
    let mut turn_output_tokens_at_start: u64 = 0;

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

    // Open cross-session DB for indexing (errors are non-fatal)
    let cross_db = crate::engine::cross_session_db::CrossSessionDb::new().ok();

    // Write the user message that was just added (last message in the vec)
    if let Some(last_msg) = messages.last() {
        append_transcript(&mut transcript_writer, &TranscriptEntry {
            timestamp: last_msg.timestamp.clone(),
            entry_type: TranscriptEntryType::UserMessage,
            data: serde_json::to_value(last_msg).unwrap_or_default(),
        });
        // Index user message for cross-session search
        if let (Some(ref db), Some(ref sid)) = (&cross_db, &config.session_id) {
            if let MessageContent::User { message, .. } = &last_msg.content {
                let text = match &message.content {
                    serde_json::Value::String(s) => s.clone(),
                    _ => serde_json::to_string(&message.content).unwrap_or_default(),
                };
                let _ = db.index_message(sid, "user", &text, &last_msg.timestamp);
            }
        }
    }

    // Build FallbackController from config
    let fallback_config = BaoclawConfig {
        model: config.model.clone(),
        fallback_models: config.fallback_models.clone(),
        max_retries_per_model: config.max_retries_per_model,
        api_type: "anthropic".to_string(),
        openai_base_url: None,
        context_window: 200_000,
        auto_compact_threshold_ratio: 0.7,
        extra: std::collections::HashMap::new(),
    };
    let mut fallback_controller = FallbackController::new(&fallback_config);

    loop {
        // Emit TurnStart immediately — user sees "Turn N" without any delay
        turn_id_counter += 1;
        turn_start_time = std::time::Instant::now();
        turn_tool_count = 0;
        turn_input_tokens_at_start = total_usage.input_tokens;
        turn_output_tokens_at_start = total_usage.output_tokens;
        let _ = tx.send(EngineEvent::TurnStart {
            turn_id: turn_id_counter,
            parent_turn_id: config.parent_turn_id,
            agent_label: config.agent_label.clone(),
        }).await;

        // Check abort (after TurnStart so CLI can handle unmatched TurnStart)
        if config.is_aborted() {
            // Clean up any orphan tool_use blocks before returning, so the
            // message history stays API-legal for the next query.
            let fixed = crate::engine::cleanup_orphan_tool_uses(messages);
            if fixed > 0 {
                eprintln!("Cleaned up {} orphan tool_use block(s) after abort", fixed);
            }
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

        // ── Git info refresh (non-blocking after first turn) ──
        // First turn or every 10th turn: refresh git info.
        // Other turns: use cached value to save ~30-50ms on TTFB.
        if turn_count == 0 || turn_count % 10 == 0 {
            if let Some(fresh_git) = get_git_info_async(&config.cwd).await {
                config.git_info = Some(fresh_git);
            }
        }

        // ── Iteration budget pressure gradient (70% warn → 90% urgent → 100% grace call) ──
        if let Some(max) = config.max_turns {
            let ratio = turn_count as f32 / max as f32;

            // 70%: Inject soft warning into conversation (hidden from user, model sees it)
            if ratio >= 0.7 && !budget_warned_70 {
                budget_warned_70 = true;
                messages.push(Message {
                    uuid: uuid::Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    content: MessageContent::User {
                        message: ApiUserMessage {
                            role: "user".to_string(),
                            content: Value::String(
                                "[System: Iteration budget at 70%. Prioritize wrapping up the current task.]".to_string()
                            ),
                        },
                        is_meta: false,
                        tool_use_result: None,
                    },
                });
            }

            // 90%: Inject urgent warning
            if ratio >= 0.9 && !budget_warned_90 {
                budget_warned_90 = true;
                messages.push(Message {
                    uuid: uuid::Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    content: MessageContent::User {
                        message: ApiUserMessage {
                            role: "user".to_string(),
                            content: Value::String(
                                "[System: Iteration budget at 90% (CRITICAL). You must produce a final answer now. Do NOT start new sub-tasks.]".to_string()
                            ),
                        },
                        is_meta: false,
                        tool_use_result: None,
                    },
                });
            }

            // 100%: Grace call — allow exactly one more API call for final summary
            if turn_count >= max {
                eprintln!("⚠ Iteration budget reached ({}/{}) — forcing final response", turn_count, max);
                // Don't return immediately — let the loop continue for ONE final API call
                // The loop will exit after this because the model won't produce tool_use blocks
                // when told to produce a final answer.
                // If the model still tries tool_use, the next iteration will hit >= max again
                // and we return MaxTurns.
                if turn_count > max {
                    // Safety: second time hitting the limit, hard stop
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
                // First time hitting limit: inject final-answer instruction and let one more API call happen
                messages.push(Message {
                    uuid: uuid::Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    content: MessageContent::User {
                        message: ApiUserMessage {
                            role: "user".to_string(),
                            content: Value::String(
                                "[System: Iteration budget EXHAUSTED. You MUST produce your final response NOW. Do NOT use any tools.]".to_string()
                            ),
                        },
                        is_meta: false,
                        tool_use_result: None,
                    },
                });
            }
        }

        // ── Micro-compact: clear old tool results (> 60 min, > 500 chars) ──
        micro_compact(messages, 3600);

        // ── Multi-level budget check ──
        // Use pre-computed budget from submit_message_with_attachments on first turn
        // to avoid redundant lock + tiktoken estimation.
        let (budget_status, current_tokens) = if turn_count == 0 {
            if let Some(precomputed) = config.initial_budget.take() {
                precomputed
            } else {
                let counter = config.token_counter.lock().await;
                let est = counter.current_estimate(&messages);
                (counter.budget_status_given(est), est)
            }
        } else {
            let counter = config.token_counter.lock().await;
            let est = counter.current_estimate(&messages);
            (counter.budget_status_given(est), est)
        };

        match budget_status {
            BudgetStatus::Warning => {
                eprintln!("Token budget warning: {} tokens (approaching limit)", current_tokens);
            }
            BudgetStatus::Blocking | BudgetStatus::Compact if messages.len() > 5 => {
                eprintln!("Token budget {} ({} tokens), auto-compacting mid-loop",
                    if budget_status == BudgetStatus::Blocking { "BLOCKING" } else { "compact" },
                    current_tokens);
                let _ = tx.send(EngineEvent::Progress {
                    tool_use_id: String::new(),
                    data: serde_json::json!({"message": format!("Context approaching limit ({} est. tokens), compacting...", current_tokens)}),
                }).await;

                // Circuit breaker: skip compact after too many consecutive failures.
                if config.compact_fail_count >= MAX_COMPACT_FAILURES {
                    eprintln!(
                        "Compact circuit breaker: {} consecutive failures, skipping",
                        config.compact_fail_count
                    );
                } else {
                    // Try session_memory_compact first (no API call needed).
                    let session_ok = config.session_memory.as_ref().map_or(false, |sm| {
                        session_memory_compact(messages, &sm.get())
                    });

                    if !session_ok {
                        match compact_messages(messages, tx.clone(), &config).await {
                            Ok(_) => {
                                eprintln!("Mid-loop auto-compact succeeded");
                                config.compact_fail_count = 0;
                            }
                            Err(e) => {
                                eprintln!("Mid-loop auto-compact failed: {}, continuing anyway", e.message);
                                config.compact_fail_count += 1;
                            }
                        }
                    } else {
                        config.compact_fail_count = 0;
                    }
                }
            }
            _ => {} // Normal
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
            token_counter: Arc::clone(&config.token_counter),
            parent_turn_id: None,
            agent_label: None,
            session_memory: config.session_memory.as_ref().map(Arc::clone),
            compact_fail_count: config.compact_fail_count,
            recent_messages_for_rules: messages.clone(),
            file_cache: config.file_cache.as_ref().map(Arc::clone),
            tool_result_store: config.tool_result_store.as_ref().map(Arc::clone),
            initial_budget: None,
            cached_rules_raw: config.cached_rules_raw.clone(),
            frozen_system_prompt: None,
            frozen_tools: None,
            frozen_hash: None,
            adaptive_compact: AdaptiveCompactTracker::new(),
            tool_health: crate::engine::tool_health::ToolHealthTracker::new(),
        };
        let request = build_api_request(&messages, &current_config);

        // Show what we're about to send
        let _ = tx.send(EngineEvent::Progress {
            tool_use_id: String::new(),
            data: serde_json::json!({
                "message": format!("Calling {} ({} messages, ~{} tokens)...",
                    current_config.model,
                    messages.len(),
                    current_tokens),
            }),
        }).await;

        // Call LLM API (streaming) with rate-limit fallback handling and timeout
        let stream_result = tokio::time::timeout(
            std::time::Duration::from_secs(300), // 5 min max per API call
            config.api_client.create_message_stream(request)
        ).await;
        let stream_result = match stream_result {
            Ok(r) => r,
            Err(_) => {
                // Remove the user message that caused the timeout so it won't
                // appear as a duplicate on the next query attempt.
                if let Some(last) = messages.last() {
                    if matches!(&last.content, MessageContent::User { .. }) {
                        eprintln!("API timeout, removing last user message to keep history clean");
                        messages.pop();
                    }
                }
                let _ = tx.send(EngineEvent::Error(EngineError {
                    code: "timeout".to_string(),
                    message: "API call timed out after 5 minutes".to_string(),
                    details: None,
                })).await;
                return;
            }
        };
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
                        let error_msg = format!(
                            "All models exhausted after {} retries. Tried: {}",
                            total_retries,
                            models_tried.join(", ")
                        );
                        if let Some(last) = messages.last() {
                            if matches!(&last.content, MessageContent::User { .. }) {
                                eprintln!("All models exhausted, removing last user message to keep history clean");
                                messages.pop();
                            }
                        }
                        let _ = tx.send(EngineEvent::Error(EngineError {
                            code: "all_models_exhausted".to_string(),
                            message: error_msg,
                            details: Some(serde_json::json!({
                                "models_tried": models_tried,
                                "total_retries": total_retries,
                            })),
                        })).await;
                        return;
                    }
                }
            }
            Err(ApiError::ServerError { status }) => {
                // Retry server errors (500, 502, 503) with exponential backoff
                const MAX_SERVER_RETRIES: u32 = 3;
                let retry_count = fallback_controller.server_error_count();
                if retry_count < MAX_SERVER_RETRIES {
                    let delay = std::time::Duration::from_millis(1000 * 2u64.pow(retry_count));
                    eprintln!(
                        "Server error {} on {}, retrying in {:?} (attempt {}/{})...",
                        status, fallback_controller.current_model(), delay, retry_count + 1, MAX_SERVER_RETRIES
                    );
                    fallback_controller.on_server_error();
                    tokio::time::sleep(delay).await;
                    continue; // retry the loop
                }
                // Exhausted retries — fall back to next model if available
                eprintln!(
                    "Server error {} on {} after {} retries, trying fallback...",
                    status, fallback_controller.current_model(), MAX_SERVER_RETRIES
                );
                match fallback_controller.on_server_error_exhausted() {
                    FallbackAction::Fallback { from, to } => {
                        let _ = tx.send(EngineEvent::ModelFallback {
                            from_model: from,
                            to_model: to,
                        }).await;
                        continue; // retry with new model
                    }
                    _ => {
                        let error_msg = format!("Server error {} after exhausting retries and fallbacks", status);
                        if let Some(last) = messages.last() {
                            if matches!(&last.content, MessageContent::User { .. }) {
                                eprintln!("Server error exhausted, removing last user message to keep history clean");
                                messages.pop();
                            }
                        }
                        let _ = tx.send(EngineEvent::Error(EngineError {
                            code: "api_server_error".to_string(),
                            message: error_msg,
                            details: None,
                        })).await;
                        return;
                    }
                }
            }
            Err(ApiError::BadRequest { message }) => {
                // 400 could be context overflow — try compaction before giving up
                let msg_lower = message.to_lowercase();
                if msg_lower.contains("context") || msg_lower.contains("token") || msg_lower.contains("too large") || msg_lower.contains("too long") {
                    eprintln!("Bad request (likely context overflow), auto-compacting...");
                    if config.compact_fail_count >= MAX_COMPACT_FAILURES {
                        eprintln!("Compact circuit breaker: {} consecutive failures, trying reactive compact", config.compact_fail_count);
                        reactive_compact(messages, None);
                        let _ = tx.send(EngineEvent::Progress {
                            tool_use_id: String::new(),
                            data: serde_json::json!({"message": "Reactive compact applied, retrying..."}),
                        }).await;
                        continue;
                    }
                    match compact_messages(messages, tx.clone(), &config).await {
                        Ok(_) => {
                            config.compact_fail_count = 0;
                            let _ = tx.send(EngineEvent::Progress {
                                tool_use_id: String::new(),
                                data: serde_json::json!({"message": "Auto-compacted context and retrying..."}),
                            }).await;
                            continue; // retry with compacted messages
                        }
                        Err(_) => {
                            config.compact_fail_count += 1;
                            // Compaction failed, try reactive compact as fallback
                            reactive_compact(messages, None);
                        }
                    }
                }
                // Clean up: remove the user message that caused the bad request,
                // so the next query doesn't send duplicate/invalid messages.
                if let Some(last) = messages.last() {
                    if matches!(&last.content, MessageContent::User { .. }) {
                        eprintln!("Bad request, removing last user message to keep history clean");
                        messages.pop();
                    }
                }
                let _ = tx.send(EngineEvent::Error(EngineError {
                    code: "api_bad_request".to_string(),
                    message: format!("{}", message),
                    details: None,
                })).await;
                return;
            }
            Err(e) => {
                // Other API errors — remove the last user message to keep history clean
                if let Some(last) = messages.last() {
                    if matches!(&last.content, MessageContent::User { .. }) {
                        eprintln!("API error, removing last user message to keep history clean");
                        messages.pop();
                    }
                }
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

        while let Some(event_result) = tokio::select! {
            result = stream.next() => result,
            // Event-driven abort: resolves immediately when abort fires,
            // vs the old 500ms polling loop.
            _ = crate::engine::wait_for_abort(config.abort_rx.clone()) => {
                eprintln!("Query aborted during stream processing");
                let fixed = crate::engine::cleanup_orphan_tool_uses(messages);
                if fixed > 0 {
                    eprintln!("Cleaned up {} orphan tool_use block(s) after stream abort", fixed);
                }
                let _ = tx.send(EngineEvent::Result(QueryResult {
                    status: QueryStatus::Aborted, text: None, stop_reason: None,
                    total_cost_usd: cost_tracker.total_cost(), usage: total_usage,
                    num_turns: turn_count, duration_ms: start_time.elapsed().as_millis() as u64,
                })).await;
                return;
            }
        } {
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
                                // Some APIs send the full input in content_block_start
                                // instead of streaming via input_json_delta. Pre-seed
                                // current_tool_input_json if a non-empty input is present.
                                current_tool_input_json = match content_block.get("input") {
                                    Some(v) if v.is_object() && !v.as_object().unwrap().is_empty() => {
                                        serde_json::to_string(v).unwrap_or_default()
                                    }
                                    _ => String::new(),
                                };
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
                                if current_tool_input_json.trim().is_empty() {
                                    eprintln!("[WARN] tool_use '{}' (id={}) has empty input_json — model returned no arguments",
                                        current_tool_name, current_tool_id);
                                }
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

                            // Calibrate the token counter against the real API-reported input_tokens.
                            // This anchors future estimates to the truth, so subsequent
                            // tiktoken-based deltas only need to count newly-added messages.
                            if start_usage.input_tokens > 0 {
                                let mut counter = config.token_counter.lock().await;
                                counter.calibrate(start_usage.input_tokens, messages.len());
                                if let Some(ref sid) = config.session_id {
                                    counter.save_baseline(sid);
                                }
                            }
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
                    // Stream error — clean up: if no assistant content was accumulated,
                    // remove the user message to keep history valid
                    if assistant_content_blocks.is_empty() {
                        if let Some(last) = messages.last() {
                            if matches!(&last.content, MessageContent::User { .. }) {
                                messages.pop();
                            }
                        }
                    }
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
        // Index assistant text for cross-session search
        if let (Some(ref db), Some(ref sid)) = (&cross_db, &config.session_id) {
            let text: String = assistant_content_blocks.iter().filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            }).collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                let _ = db.index_message(sid, "assistant", &text, &assistant_msg.timestamp);
            }
        }

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
                    let mut split = messages.len() - keep_recent;

                    // Ensure we don't split between tool calls and their results.
                    // Handle cases where one assistant message has multiple tool_use blocks.
                    if split > 0 && split < messages.len() {
                        if let MessageContent::Assistant { message, .. } = &messages[split - 1].content {
                            // Extract all tool_use IDs from the assistant message
                            let tool_use_ids: Vec<&str> = message.content.iter()
                                .filter_map(|block| match block {
                                    ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                                    _ => None,
                                })
                                .collect();

                            if !tool_use_ids.is_empty() {
                                // Scan forward to find all corresponding tool_result messages
                                let mut found_results: std::collections::HashSet<String> = std::collections::HashSet::new();
                                let mut next_idx = split;

                                while next_idx < messages.len() {
                                    if let MessageContent::User { message, .. } = &messages[next_idx].content {
                                        let result_ids = extract_tool_result_ids(message);
                                        for id in result_ids {
                                            if tool_use_ids.contains(&id.as_str()) {
                                                found_results.insert(id);
                                            }
                                        }
                                        // Stop if we've found all tool_use results
                                        if found_results.len() == tool_use_ids.len() {
                                            break;
                                        }
                                    }
                                    next_idx += 1;
                                }

                                // Adjust split to include all tool_result messages in old_messages
                                // Otherwise, move the assistant message to recent_messages
                                if found_results.len() == tool_use_ids.len() {
                                    split = next_idx + 1;
                                } else {
                                    // Not all results found - move assistant message to recent_messages
                                    if split > 1 {
                                        split -= 1;
                                    }
                                }
                            }
                        }
                    }

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
                    let compact_abort_rx = config.abort_rx.clone();
                    let compact_api_client = Arc::clone(&config.api_client);
                    let compact_model = config.model.clone();
                    let summary_result = async move {
                        let mut stream = compact_api_client.create_message_stream(summary_request).await
                            .map_err(|e| format!("{}", e))?;
                        let mut text = String::new();
                        let abort_rx = compact_abort_rx;
                        loop {
                            let event_result = tokio::select! {
                                r = stream.next() => r,
                                _ = crate::engine::wait_for_abort(abort_rx.clone()) => {
                                    eprintln!("Aborted during compact summary streaming");
                                    break;
                                }
                            };
                            let Some(event_result) = event_result else { break; };
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

                            // If still too many messages, drop oldest turns
                            // as a last-resort escape.
                            if messages.len() > 10 {
                                reactive_compact(messages, None);
                            }
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
            // Emit TurnEnd for the final turn (no tools)
            let _ = tx.send(EngineEvent::TurnEnd {
                turn_id: turn_id_counter,
                duration_ms: turn_start_time.elapsed().as_millis() as u64,
                tool_count: turn_tool_count,
                input_tokens: total_usage.input_tokens.saturating_sub(turn_input_tokens_at_start),
                output_tokens: total_usage.output_tokens.saturating_sub(turn_output_tokens_at_start),
            }).await;
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
            turn_tool_count += 1;
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
            file_cache: config.file_cache.as_ref().map(Arc::clone),
            tool_result_store: config.tool_result_store.as_ref().map(Arc::clone),
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

        // Emit TurnEnd after tool results are processed
        let _ = tx.send(EngineEvent::TurnEnd {
            turn_id: turn_id_counter,
            duration_ms: turn_start_time.elapsed().as_millis() as u64,
            tool_count: turn_tool_count,
            input_tokens: total_usage.input_tokens.saturating_sub(turn_input_tokens_at_start),
            output_tokens: total_usage.output_tokens.saturating_sub(turn_output_tokens_at_start),
        }).await;

        turn_count += 1;

        // ── Background session memory update ──
        // Fire-and-forget: spawn a background task to update the session summary
        // every N turns.  The summary is persisted to .memory.md and loaded
        // instantly on next session startup.
        if let Some(ref sm) = config.session_memory {
            let current_count = messages.len();
            if sm.should_update(current_count) {
                let msgs_clone = messages.clone();
                let sm_arc = Arc::clone(sm);
                let api = Arc::clone(&config.api_client);
                let mdl = config.model.clone();
                let existing = sm.get();
                tokio::spawn(async move {
                    update_session_memory_background(
                        msgs_clone, sm_arc, api, mdl, existing,
                    ).await;
                });
            }
        }
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

/// A parsed rule file from `.baoclaw/rules/*.md`.
struct RuleFile {
    /// Rule content (with YAML frontmatter stripped).
    content: String,
    /// Optional glob pattern from frontmatter `paths` field.
    paths_pattern: Option<String>,
}

/// Load rules from `.baoclaw/rules/*.md`, optionally filtering by `recent_file_paths`.
///
/// Rules without a `paths` frontmatter field are loaded unconditionally.
/// Rules with `paths` are only included when at least one entry in
/// `recent_file_paths` matches the glob pattern.
pub fn load_rules_with_paths(cwd: &Path, recent_file_paths: &[String]) -> Vec<String> {
    let rules_dir = cwd.join(".baoclaw").join("rules");
    let entries = match std::fs::read_dir(&rules_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut matched_rules: Vec<String> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rule = parse_rule_file(&content);
        let should_include = match &rule.paths_pattern {
            None => true,
            Some(pattern) => {
                // Use glob matching: include if any recent file path matches
                match glob::Pattern::new(pattern) {
                    Ok(glob_pattern) => {
                        recent_file_paths.iter().any(|fp| {
                            // Try matching against the full path or just the filename
                            glob_pattern.matches(fp) || glob_pattern.matches(std::path::Path::new(fp).file_name().and_then(|n| n.to_str()).unwrap_or(""))
                        })
                    }
                    Err(e) => {
                        eprintln!("Warning: invalid glob pattern '{}' in {}: {}", pattern, path.display(), e);
                        // If pattern is invalid, include the rule anyway
                        true
                    }
                }
            }
        };

        if should_include && !rule.content.trim().is_empty() {
            matched_rules.push(format!(
                "# Rule: {}\n\n{}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                rule.content
            ));
        }
    }

    matched_rules
}

/// Parse a rule file, extracting YAML frontmatter if present.
///
/// Supports `---` delimited frontmatter with a `paths` field:
/// ```markdown
/// ---
/// paths: "src/**/*.rs"
/// ---
/// Rule content here.
/// ```
fn parse_rule_file(content: &str) -> RuleFile {
    let trimmed = content.trim();

    // Check for YAML frontmatter
    if trimmed.starts_with("---") {
        // Find closing ---
        if let Some(rest) = trimmed.get(3..) {
            if let Some(end_idx) = rest.find("---") {
                let frontmatter = &rest[..end_idx];
                let body = rest[end_idx + 3..].trim();

                // Parse paths from frontmatter (simple line-based parsing)
                let paths_pattern = frontmatter
                    .lines()
                    .find_map(|line| {
                        let line = line.trim();
                        if line.starts_with("paths:") || line.starts_with("paths :") {
                            let value = line.splitn(2, ':').nth(1)?.trim();
                            // Strip quotes if present
                            let value = value.trim_matches('"').trim_matches('\'');
                            if value.is_empty() {
                                None
                            } else {
                                Some(value.to_string())
                            }
                        } else {
                            None
                        }
                    });

                return RuleFile {
                    content: body.to_string(),
                    paths_pattern,
                };
            }
        }
    }

    // No frontmatter
    RuleFile {
        content: trimmed.to_string(),
        paths_pattern: None,
    }
}

/// Load all rule files from `.baoclaw/rules/*.md` into cached structures.
/// This is called once in `QueryEngine::new()` and the results are reused
/// across turns, avoiding repeated file I/O.
fn load_all_rule_files(cwd: &Path) -> Vec<CachedRule> {
    let rules_dir = cwd.join(".baoclaw").join("rules");
    let entries = match std::fs::read_dir(&rules_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut rules = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rule = parse_rule_file(&content);
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
        rules.push(CachedRule {
            filename,
            content: rule.content,
            paths_pattern: rule.paths_pattern,
        });
    }
    rules
}

/// Filter cached rules against recent file paths using glob matching.
/// Replaces `load_rules_with_paths` for in-memory cached rules.
fn filter_cached_rules(cached: &[CachedRule], recent_file_paths: &[String]) -> Vec<String> {
    cached.iter()
        .filter(|rule| {
            match &rule.paths_pattern {
                None => true,
                Some(pattern) => {
                    match glob::Pattern::new(pattern) {
                        Ok(glob_pattern) => {
                            recent_file_paths.iter().any(|fp| {
                                glob_pattern.matches(fp) || glob_pattern.matches(
                                    std::path::Path::new(fp).file_name().and_then(|n| n.to_str()).unwrap_or("")
                                )
                            })
                        }
                        Err(_) => true, // Invalid pattern → include anyway
                    }
                }
            }
        })
        .filter(|rule| !rule.content.trim().is_empty())
        .map(|rule| format!("# Rule: {}\n\n{}", rule.filename, rule.content))
        .collect()
}

/// Extract file paths mentioned in the most recent N messages.
///
/// Looks for file_path, file, path, pattern, cwd, and directory fields in
/// tool inputs and text content.
fn extract_recent_file_paths(messages: &[Message], max_messages: usize) -> Vec<String> {
    let start = messages.len().saturating_sub(max_messages);
    let mut paths = Vec::new();

    for msg in &messages[start..] {
        match &msg.content {
            MessageContent::User { message, .. } => {
                // Look for file_path in tool_result content
                if let Value::Array(blocks) = &message.content {
                    for block in blocks {
                        if let Some(content) = block.get("content").and_then(|c| c.as_str()) {
                            // Extract file paths that appear in tool results
                            for line in content.lines().take(50) {
                                if line.contains('/') || line.contains("\\.") {
                                    paths.push(line.trim().to_string());
                                }
                            }
                        }
                    }
                }
                if let Value::String(text) = &message.content {
                    // Simple heuristic: extract path-like strings from text
                    for word in text.split_whitespace() {
                        if (word.contains('/') || word.contains(".rs") || word.contains(".ts")
                            || word.contains(".js") || word.contains(".py")
                            || word.contains(".md") || word.contains(".toml"))
                            && word.len() > 5 && word.len() < 300
                        {
                            paths.push(word.to_string());
                        }
                    }
                }
            }
            MessageContent::Assistant { message, .. } => {
                for block in &message.content {
                    if let ContentBlock::ToolUse { input, .. } = block {
                        // Extract file_path from tool inputs
                        if let Some(fp) = input.get("file_path").and_then(|v| v.as_str()) {
                            paths.push(fp.to_string());
                        }
                        if let Some(p) = input.get("path").and_then(|v| v.as_str()) {
                            paths.push(p.to_string());
                        }
                        if let Some(p) = input.get("pattern").and_then(|v| v.as_str()) {
                            paths.push(p.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));

    // Limit to reasonable count
    paths.truncate(50);
    paths
}

/// Compact messages in-place: summarize old messages via API and replace with a boundary.
/// Used both for preemptive auto-compaction and for recovery after context-overflow errors.
async fn compact_messages(
    messages: &mut Vec<Message>,
    tx: mpsc::Sender<EngineEvent>,
    config: &QueryLoopConfig,
) -> Result<(), EngineError> {
    const KEEP_RECENT: usize = 10; // keep last 10 messages (5 turns)
    if messages.len() <= KEEP_RECENT {
        return Ok(());
    }

    let mut old_count = messages.len() - KEEP_RECENT;

    // Ensure we don't split between tool calls and their results.
    // Handle cases where one assistant message has multiple tool_use blocks.
    if old_count > 0 && old_count < messages.len() {
        if let MessageContent::Assistant { message, .. } = &messages[old_count - 1].content {
            // Extract all tool_use IDs from the assistant message
            let tool_use_ids: Vec<&str> = message.content.iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                    _ => None,
                })
                .collect();

            if !tool_use_ids.is_empty() {
                // Scan forward to find all corresponding tool_result messages
                let mut found_results: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut next_idx = old_count;

                while next_idx < messages.len() {
                    if let MessageContent::User { message, .. } = &messages[next_idx].content {
                        let result_ids = extract_tool_result_ids(message);
                        for id in result_ids {
                            if tool_use_ids.contains(&id.as_str()) {
                                found_results.insert(id);
                            }
                        }
                        // Stop if we've found all tool_use results
                        if found_results.len() == tool_use_ids.len() {
                            break;
                        }
                    }
                    next_idx += 1;
                }

                // Adjust old_count to include all tool_result messages
                // Otherwise, move the assistant message to recent_messages
                if found_results.len() == tool_use_ids.len() {
                    old_count = next_idx + 1;
                } else {
                    // Not all results found - move assistant message to recent_messages
                    if old_count > 1 {
                        old_count -= 1;
                    }
                }
            }
        }
    }

    // Clone old messages to avoid borrowing messages during API call
    let old_messages: Vec<Message> = messages[..old_count].to_vec();
    let recent_messages: Vec<Message> = messages[old_count..].to_vec();

    let raw_summary = format_messages_for_summary(&old_messages);
    let max_summary_chars: usize = 60_000;
    let truncated_summary = if raw_summary.len() > max_summary_chars {
        format!("{}...\n\n[Conversation truncated, {} total chars]",
            &raw_summary.chars().take(max_summary_chars).collect::<String>(), raw_summary.len())
    } else {
        raw_summary
    };
    let summary_instruction = format!(
        "Summarize the following conversation history concisely, \
         preserving key context, decisions, and file changes:\n\n{}",
        truncated_summary
    );

    // Cache-Safe Forking: build the compaction request using the EXACT SAME
    // system prompt, tools, and conversation history as the main dialogue.
    // This ensures the API can reuse the cached prefix from the main session,
    // and only pays for the new summarisation message at the end.
    //
    // Old approach (broken): separate system prompt ("You are a summariser")
    // + no tools + no history → zero cache reuse, full price every time.
    let main_request = build_api_request(messages, config);
    let old_api_messages: Vec<serde_json::Value> = old_messages.iter().filter_map(|msg| {
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
    let request = CreateMessageRequest::for_cache_safe_compaction(
        &main_request,
        &old_api_messages,
        &summary_instruction,
    );

    let stream_result = config.api_client.create_message_stream(request).await;
    let mut stream = match stream_result {
        Ok(s) => s,
        Err(e) => {
            return Err(EngineError {
                code: "compact_failed".to_string(),
                message: format!("Failed to call summary API: {}", e),
                details: None,
            });
        }
    };

    let mut summary_text = String::new();
    loop {
        let event_result = tokio::select! {
            r = stream.next() => r,
            _ = crate::engine::wait_for_abort(config.abort_rx.clone()) => {
                eprintln!("Compact aborted by user");
                return Err(EngineError {
                    code: "compact_aborted".to_string(),
                    message: "User aborted compaction".to_string(),
                    details: None,
                });
            }
        };
        let Some(event_result) = event_result else { break; };
        match event_result {
            Ok(event) => {
                if let ApiStreamEvent::ContentBlockDelta { delta, .. } = event {
                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                        summary_text.push_str(text);
                    }
                }
            }
            Err(e) => {
                eprintln!("Compact: stream error: {}", e);
                break;
            }
        }
    }

    if summary_text.trim().is_empty() {
        summary_text = format!("[Previous conversation ({} messages) was compacted]", old_count);
    }

    let boundary = Message {
        uuid: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content: MessageContent::System {
            subtype: crate::models::message::SystemSubtype::CompactBoundary,
            content: summary_text,
        },
    };

    *messages = vec![boundary];
    messages.extend(recent_messages);

    let _ = tx.send(EngineEvent::Progress {
        tool_use_id: String::new(),
        data: serde_json::json!({"message": format!("Context compacted: {} messages summarized", old_count)}),
    }).await;

    Ok(())
}

/// Maximum consecutive compact failures before the circuit breaker trips.
const MAX_COMPACT_FAILURES: usize = 3;

/// Micro-compact: replace old, large tool-result content with placeholders.
///
/// Called before each API call in the query loop.  Tool results older than
/// `idle_threshold_secs` (default 60 min) and larger than 500 chars are
/// replaced with a size annotation, freeing context budget without an API
/// summarisation round-trip.
fn micro_compact(messages: &mut Vec<Message>, idle_threshold_secs: u64) {
    let now = std::time::SystemTime::now();
    let threshold = std::time::Duration::from_secs(idle_threshold_secs);

    // Skip the last few messages (they are the current turn — keep intact).
    let skip_recent = 4usize;
    let start = messages.len().saturating_sub(skip_recent);

    for msg in messages[..start].iter_mut() {
        // Compute age from the message timestamp.
        let age = match chrono::DateTime::parse_from_rfc3339(&msg.timestamp) {
            Ok(ts) => {
                let msg_time = std::time::SystemTime::from(ts.with_timezone(&chrono::Utc));
                now.duration_since(msg_time).unwrap_or(std::time::Duration::ZERO)
            }
            Err(_) => continue,
        };

        if age < threshold {
            continue;
        }

        // Replace large tool-result payloads with a placeholder.
        if let MessageContent::User { message, .. } = &mut msg.content {
            if let Value::Array(blocks) = &mut message.content {
                for block in blocks.iter_mut() {
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                        if let Some(content) = block.get_mut("content") {
                            let output_str = content.to_string();
                            if output_str.len() > 500 {
                                *content = serde_json::json!(
                                    format!("[Old tool result cleared — originally {} chars]", output_str.len())
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Lightweight compact path using the session memory.
///
/// If a session memory summary already exists, use it as the CompactBoundary
/// and keep only the most recent messages — no API summarisation call needed.
pub(crate) fn session_memory_compact(
    messages: &mut Vec<Message>,
    summary_text: &str,
) -> bool {
    if summary_text.is_empty() {
        return false;
    }

    let keep_recent: usize = 10;
    if messages.len() <= keep_recent {
        return false;
    }

    let boundary = Message {
        uuid: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content: MessageContent::System {
            subtype: crate::models::message::SystemSubtype::CompactBoundary,
            content: format!("[Session Memory]\n{}", summary_text),
        },
    };

    let recent = messages[messages.len() - keep_recent..].to_vec();
    *messages = vec![boundary];
    messages.extend(recent);

    eprintln!(
        "Session-memory compact: replaced {} old messages, kept {} recent",
        messages.len() - keep_recent - 1,
        keep_recent
    );
    true
}

/// Reactive compact — drop the oldest turns when all other compaction has
/// failed and we still can't fit the context window.
///
/// Groups messages into assistant+user "turns" and drops the oldest 20% (or
/// enough to hit `target_reduction` estimated tokens).
fn reactive_compact(messages: &mut Vec<Message>, target_reduction: Option<usize>) {
    if messages.len() <= 4 {
        return;
    }

    // Group into turns: each turn starts with a user message and includes
    // the following assistant message (and any tool-result user messages).
    let mut turn_starts: Vec<usize> = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        match &msg.content {
            MessageContent::User { message, tool_use_result, .. } => {
                // A turn-starting user message is one that is NOT a tool_result.
                if tool_use_result.is_none() {
                    // Also skip if content is an array of tool_result blocks.
                    let is_tool_result_array = match &message.content {
                        Value::Array(arr) => arr.iter().all(|b| {
                            b.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                        }),
                        _ => false,
                    };
                    if !is_tool_result_array {
                        turn_starts.push(i);
                    }
                }
            }
            _ => {}
        }
    }

    if turn_starts.len() <= 2 {
        // Not enough turns to drop.
        return;
    }

    let drop_count = match target_reduction {
        Some(target) => {
            // Estimate tokens per turn (rough: total / turn_count).
            let total_tokens = estimate_tokens(messages) as usize;
            let tokens_per_turn = total_tokens / turn_starts.len().max(1);
            (target / tokens_per_turn.max(1)).max(1).min(turn_starts.len() / 2)
        }
        None => (turn_starts.len() / 5).max(1), // default: drop 20%
    };

    if drop_count >= turn_starts.len() {
        return;
    }

    let drop_to = turn_starts[drop_count];
    eprintln!(
        "Reactive compact: dropping {} oldest turns ({} messages)",
        drop_count,
        drop_to
    );
    *messages = messages[drop_to..].to_vec();
}

/// Validate and fix tool_use/tool_result pairing in messages before API call.
/// This ensures we never send malformed messages to the API.
fn validate_and_fix_tool_messages(messages: &[Message]) -> Vec<Message> {
    eprintln!("=== validate_and_fix_tool_messages: START ===");
    eprintln!("  Input messages: {}", messages.len());

    // First pass: collect all tool_use IDs and their corresponding tool_result IDs
    let mut tool_use_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut tool_result_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (idx, msg) in messages.iter().enumerate() {
        match &msg.content {
            MessageContent::Assistant { message, .. } => {
                let ids: Vec<String> = message.content.iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                        _ => None,
                    })
                    .collect();
                for id in ids {
                    tool_use_ids.insert(id);
                }
            }
            MessageContent::User { message, .. } => {
                let ids = extract_tool_result_ids(message);
                for id in ids {
                    tool_result_ids.insert(id);
                }
            }
            _ => {}
        }
    }

    eprintln!("  Found {} tool_use IDs: {:?}", tool_use_ids.len(), tool_use_ids);
    eprintln!("  Found {} tool_result IDs: {:?}", tool_result_ids.len(), tool_result_ids);

    // Find orphaned tool_use IDs (without corresponding tool_result)
    let orphaned_tool_uses: std::collections::HashSet<String> = tool_use_ids
        .difference(&tool_result_ids)
        .cloned()
        .collect();

    // Find orphaned tool_result IDs (without corresponding tool_use)
    let orphaned_tool_results: std::collections::HashSet<String> = tool_result_ids
        .difference(&tool_use_ids)
        .cloned()
        .collect();

    eprintln!("  Orphaned tool_use blocks: {}", orphaned_tool_uses.len());
    eprintln!("  Orphaned tool_result blocks: {}", orphaned_tool_results.len());

    // Filter messages: remove those with orphaned tool_use/tool_result
    let mut result = Vec::new();
    for msg in messages {
        match &msg.content {
            MessageContent::System { .. } => {
                // Skip system messages (CompactBoundary etc.)
                continue;
            }
            MessageContent::Assistant { message, .. } => {
                // Check if this assistant message contains any orphaned tool_use
                let has_orphaned = message.content.iter().any(|block| {
                    if let ContentBlock::ToolUse { id, .. } = block {
                        orphaned_tool_uses.contains(id)
                    } else {
                        false
                    }
                });
                if !has_orphaned {
                    result.push(msg.clone());
                } else {
                    eprintln!("validate_and_fix: skipping assistant message with orphaned tool_use");
                }
            }
            MessageContent::User { message, .. } => {
                // Check if this user message contains only orphaned tool_result
                let result_ids = extract_tool_result_ids(message);
                if result_ids.is_empty() {
                    // Regular user message, keep it
                    result.push(msg.clone());
                } else {
                    // Tool result message - check if any results are valid (not orphaned)
                    let has_valid_result = result_ids.iter().any(|id| !orphaned_tool_results.contains(id));
                    if has_valid_result {
                        // Keep message but filter out orphaned results
                        // For now, just keep the whole message
                        result.push(msg.clone());
                    } else {
                        eprintln!("validate_and_fix: skipping user message with only orphaned tool_result");
                    }
                }
            }
            _ => {
                result.push(msg.clone());
            }
        }
    }

    eprintln!("  Output messages: {}", result.len());
    eprintln!("=== validate_and_fix_tool_messages: END ===");

    result
}

/// Build an API request from the current messages and config.
fn build_api_request(messages: &[Message], config: &QueryLoopConfig) -> CreateMessageRequest {
    // First validate and fix tool_use/tool_result pairing
    let validated_messages = validate_and_fix_tool_messages(messages);

    // Convert messages to API format
    let mut api_messages: Vec<Value> = validated_messages.iter().filter_map(|msg| {
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

    // Inject dynamic <system-reminder> into the last user message to avoid
    // invalidating the cached system prompt prefix.  Git status, session
    // memory, and other per-turn information goes here.
    if let Some(reminder) = build_dynamic_reminder(config) {
        if let Some(last_msg) = api_messages.last_mut() {
            // Append the reminder to the existing user message content
            if let Some(content) = last_msg.get_mut("content") {
                match content {
                    Value::String(s) => {
                        *s = format!("{}\n\n{}", s, reminder);
                    }
                    Value::Array(blocks) => {
                        blocks.push(serde_json::json!({
                            "type": "text",
                            "text": reminder,
                        }));
                    }
                    _ => {
                        // Fallback: replace content with a string containing the reminder
                        *content = Value::String(format!("{}\n\n{}", content, reminder));
                    }
                }
            }
        }
    }

    // Use frozen system prompt if available (maximizes cache hit rate)
    let system = if let Some(ref frozen) = config.frozen_system_prompt {
        Some(frozen.clone())
    } else {
        build_system_prompt(config)
    };

    // Use frozen tools list if available (same caching benefit)
    let tools = if let Some(ref frozen) = config.frozen_tools {
        Some(frozen.clone())
    } else {
        build_tools_list(config)
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

/// Build the tools list deterministically — called once and frozen for the session.
///
/// Tool order is part of the cached prefix, so non-deterministic iteration
/// (e.g. HashMap-based) would break caching.
fn build_tools_list(config: &QueryLoopConfig) -> Option<Vec<Value>> {
    if config.tools.is_empty() {
        return None;
    }
    let mut tool_list: Vec<Value> = config.tools.iter().map(|t| {
        if t.is_deferred() {
            serde_json::json!({
                "name": t.name(),
                "description": t.short_description(),
                "defer_loading": true,
            })
        } else {
            let schema = t.input_schema();
            serde_json::json!({
                "name": t.name(),
                "description": t.prompt(),
                "input_schema": schema,
            })
        }
    }).collect();
    tool_list.sort_by(|a, b| {
        let name_a = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let name_b = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        name_a.cmp(name_b)
    });
    if let Some(last_tool) = tool_list.last_mut() {
        last_tool.as_object_mut().map(|obj| {
            obj.insert(
                "cache_control".to_string(),
                serde_json::json!({ "type": "ephemeral" }),
            );
        });
    }
    Some(tool_list)
}

/// Build the system prompt from config — **static parts only**.
///
/// Prompt Caching works via prefix matching: any change to the system prompt
/// invalidates the entire cached prefix.  Therefore, only content that is
/// stable across turns is placed here.  Dynamic information (git status,
/// session memory) is injected via `<system-reminder>` user messages instead.
///
/// Order (stable → volatile):
///   1. Core system prompt / custom prompt      ← never changes in-session
///   2. Working directory                        ← never changes in-session
///   3. Project instructions (BAOCLAW.md)        ← rarely changes
///   4. Project rules (.baoclaw/rules/)          ← rarely changes
///   5. Append system prompt                     ← rarely changes
pub fn build_system_prompt(config: &QueryLoopConfig) -> Option<Vec<Value>> {
    let mut parts: Vec<String> = Vec::new();

    // 1. Core system prompt
    if let Some(custom) = &config.custom_system_prompt {
        parts.push(custom.clone());
    } else {
        parts.push("You are a helpful AI coding assistant.".to_string());
    }

    // 2. Current working directory (stable within a session)
    parts.push(format!(
        "Current working directory: {}\n\nWhen the user asks to display or show a file's content, output the full content directly in your response. Do not summarize or describe the file — show the actual text.",
        config.cwd.display()
    ));

    // 3. Project instructions from BAOCLAW.md (rarely changes mid-session)
    if let Some(instructions) = &config.project_instructions {
        parts.push(format!(
            "# Project Instructions (from BAOCLAW.md)\n\n{}",
            instructions
        ));
    }

    // 4. Project rules from .baoclaw/rules/*.md (path-filtered from cache)
    {
        let recent_paths = extract_recent_file_paths(&config.recent_messages_for_rules, 10);
        let rules = filter_cached_rules(&config.cached_rules_raw, &recent_paths);
        if !rules.is_empty() {
            parts.push(format!(
                "# Project Rules (from .baoclaw/rules/)\n\n{}",
                rules.join("\n\n")
            ));
        }
    }

    // 5. Append system prompt
    if let Some(append) = &config.append_system_prompt {
        parts.push(append.clone());
    }

    if parts.is_empty() {
        None
    } else {
        let combined = parts.join("\n\n");
        // Mark the static system prompt with cache_control so the API caches it.
        Some(vec![serde_json::json!({
            "type": "text",
            "text": combined,
            "cache_control": { "type": "ephemeral" },
        })])
    }
}

/// Build a `<system-reminder>` user message containing **dynamic** information
/// that changes between turns (git status, session memory, etc.).
///
/// This content is kept out of the system prompt so that the cached prefix
/// remains stable.  The reminder is appended as a user message — the model
/// still sees it, but it doesn't invalidate the prompt cache.
pub fn build_dynamic_reminder(config: &QueryLoopConfig) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // Git status — changes every turn as files are edited
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

    // Session memory (rolling summary) — updated after compaction
    if let Some(sm) = &config.session_memory {
        let memory = sm.get();
        if !memory.is_empty() {
            parts.push(format!("# Session Memory\n\n{}", memory));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("<system-reminder>\n{}\n</system-reminder>", parts.join("\n\n")))
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

/// Extract tool result IDs from a user message content.
/// Returns a list of tool_use_id values found in tool_result blocks.
fn extract_tool_result_ids(user_message: &ApiUserMessage) -> Vec<String> {
    let mut ids = Vec::new();

    match &user_message.content {
        Value::Array(arr) => {
            for block in arr {
                if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                    if block_type == "tool_result" {
                        if let Some(id) = block.get("tool_use_id").and_then(|v| v.as_str()) {
                            ids.push(id.to_string());
                        }
                    }
                }
            }
        }
        _ => {}
    }

    ids
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
    // Max chars per tool result to avoid exceeding API limits (especially for OpenAI-compatible APIs)
    const MAX_TOOL_RESULT_CHARS: usize = 30_000;

    let content_blocks: Vec<Value> = results.iter().map(|r| {
        // Strip large base64 image data from tool output to avoid bloating context
        let raw_output = strip_base64_images(&r.output);
        // API requires content to be a string or array of content blocks, not an object
        let content = match &raw_output {
            Value::String(s) => {
                if s.len() > MAX_TOOL_RESULT_CHARS {
                    Value::String(format!(
                        "{}\n\n[… truncated, {} total chars]",
                        &s.chars().take(MAX_TOOL_RESULT_CHARS).collect::<String>(),
                        s.len()
                    ))
                } else {
                    Value::String(s.clone())
                }
            }
            Value::Null => Value::String(String::new()),
            Value::Array(arr) => Value::Array(arr.clone()),
            other => {
                let s = serde_json::to_string(other).unwrap_or_default();
                if s.len() > MAX_TOOL_RESULT_CHARS {
                    Value::String(format!(
                        "{}\n\n[… truncated, {} total chars]",
                        &s.chars().take(MAX_TOOL_RESULT_CHARS).collect::<String>(),
                        s.len()
                    ))
                } else {
                    Value::String(s)
                }
            }
        };
        serde_json::json!({
            "type": "tool_result",
            "tool_use_id": r.tool_use_id,
            "content": content,
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

/// Background task: generate a session summary and persist it.
///
/// Spawned (fire-and-forget) after each query loop iteration when
/// `session_memory.should_update()` returns true.  Uses a lightweight
/// API call (no tools, no conversation history) to generate a rolling
/// summary that is loaded instantly on next session startup.
async fn update_session_memory_background(
    messages: Vec<Message>,
    session_memory: Arc<SessionMemory>,
    api_client: Arc<UnifiedClient>,
    model: String,
    existing_summary: String,
) {
    let msg_count = messages.len();
    if msg_count < 4 {
        return;
    }

    let conversation_text = format_messages_for_summary(&messages);
    let max_chars: usize = 40_000;
    let truncated = if conversation_text.len() > max_chars {
        format!("{}...\n\n[Truncated, {} total chars]",
            &conversation_text.chars().take(max_chars).collect::<String>(),
            conversation_text.len())
    } else {
        conversation_text
    };

    let prompt = if existing_summary.is_empty() {
        format!(
            "You are summarizing a coding assistant session. Create a concise meeting-notes summary of the conversation so far.\n\
             Preserve: key decisions, file changes, errors encountered, current task status, and any pending items.\n\
             Format as markdown with headers.\n\n\
             Conversation:\n{}", truncated)
    } else {
        format!(
            "You are updating a rolling summary of a coding assistant session.\n\
             Here is the existing summary:\n---\n{}\n---\n\n\
             Here is the recent conversation:\n{}\n\n\
             Update the summary to reflect the latest work. Preserve key decisions, file changes, and pending items.",
            existing_summary, truncated)
    };

    let request = CreateMessageRequest {
        model,
        messages: vec![serde_json::json!({
            "role": "user",
            "content": prompt,
        })],
        system: Some(vec![serde_json::json!({
            "type": "text",
            "text": "You are a session summarizer. Produce concise, structured markdown summaries.",
            "cache_control": {"type": "ephemeral"},
        })]),
        tools: None,
        max_tokens: 2048,
        stream: true,
        thinking: None,
        metadata: None,
    };

    match api_client.create_message_stream(request).await {
        Ok(mut stream) => {
            use futures::StreamExt;
            let mut summary_text = String::new();
            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(ApiStreamEvent::ContentBlockDelta { delta, .. }) => {
                        if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                            summary_text.push_str(text);
                        }
                    }
                    Err(e) => {
                        eprintln!("Session memory background update stream error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            if !summary_text.trim().is_empty() {
                session_memory.update(summary_text);
                session_memory.set_message_count(msg_count);
                eprintln!("Session memory updated ({} chars, at msg #{})",
                    session_memory.get().len(), msg_count);
            }
        }
        Err(e) => {
            eprintln!("Session memory background update failed: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::ApiClientConfig;
    use crate::models::message::ContentBlock;
    use serde_json::json;

    fn make_config() -> QueryEngineConfig {
        let api_client = Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
            api_key: "test-key".to_string(),
            base_url: None,
            max_retries: None,
            api_path: None,
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
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
            parent_turn_id: None,
            agent_label: None,
            session_memory: None,
            file_cache: None,
            tool_result_store: None,
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
            api_path: None,
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
            token_counter: Arc::new(tokio::sync::Mutex::new(crate::engine::token_counter::TokenCounter::new(200_000, 0.7))),
            parent_turn_id: None,
            agent_label: None,
            session_memory: None,
            compact_fail_count: 0,
            recent_messages_for_rules: vec![],
            file_cache: None,
            tool_result_store: None,
            initial_budget: None,
            cached_rules_raw: vec![],
            frozen_system_prompt: None,
            frozen_tools: None,
            frozen_hash: None,
            adaptive_compact: AdaptiveCompactTracker::new(),
            tool_health: crate::engine::tool_health::ToolHealthTracker::new(),
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
            api_path: None,
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
            token_counter: Arc::new(tokio::sync::Mutex::new(crate::engine::token_counter::TokenCounter::new(200_000, 0.7))),
            parent_turn_id: None,
            agent_label: None,
            session_memory: None,
            compact_fail_count: 0,
            recent_messages_for_rules: vec![],
            file_cache: None,
            tool_result_store: None,
            initial_budget: None,
            cached_rules_raw: vec![],
            frozen_system_prompt: None,
            frozen_tools: None,
            frozen_hash: None,
            adaptive_compact: AdaptiveCompactTracker::new(),
            tool_health: crate::engine::tool_health::ToolHealthTracker::new(),
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
            api_path: None,
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
            token_counter: Arc::new(tokio::sync::Mutex::new(crate::engine::token_counter::TokenCounter::new(200_000, 0.7))),
            parent_turn_id: None,
            agent_label: None,
            session_memory: None,
            compact_fail_count: 0,
            recent_messages_for_rules: vec![],
            file_cache: None,
            tool_result_store: None,
            initial_budget: None,
            cached_rules_raw: vec![],
            frozen_system_prompt: None,
            frozen_tools: None,
            frozen_hash: None,
            adaptive_compact: AdaptiveCompactTracker::new(),
            tool_health: crate::engine::tool_health::ToolHealthTracker::new(),
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
            api_path: None,
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
            token_counter: Arc::new(tokio::sync::Mutex::new(crate::engine::token_counter::TokenCounter::new(200_000, 0.7))),
            parent_turn_id: None,
            agent_label: None,
            session_memory: None,
            compact_fail_count: 0,
            recent_messages_for_rules: vec![],
            file_cache: None,
            tool_result_store: None,
            initial_budget: None,
            cached_rules_raw: vec![],
            frozen_system_prompt: None,
            frozen_tools: None,
            frozen_hash: None,
            adaptive_compact: AdaptiveCompactTracker::new(),
            tool_health: crate::engine::tool_health::ToolHealthTracker::new(),
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
            api_path: None,
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
            token_counter: Arc::new(tokio::sync::Mutex::new(crate::engine::token_counter::TokenCounter::new(200_000, 0.7))),
            parent_turn_id: None,
            agent_label: None,
            session_memory: None,
            compact_fail_count: 0,
            recent_messages_for_rules: vec![],
            file_cache: None,
            tool_result_store: None,
            initial_budget: None,
            cached_rules_raw: vec![],
            frozen_system_prompt: None,
            frozen_tools: None,
            frozen_hash: None,
            adaptive_compact: AdaptiveCompactTracker::new(),
            tool_health: crate::engine::tool_health::ToolHealthTracker::new(),
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
            api_path: None,
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
            token_counter: Arc::new(tokio::sync::Mutex::new(crate::engine::token_counter::TokenCounter::new(200_000, 0.7))),
            parent_turn_id: None,
            agent_label: None,
            session_memory: None,
            compact_fail_count: 0,
            recent_messages_for_rules: vec![],
            file_cache: None,
            tool_result_store: None,
            initial_budget: None,
            cached_rules_raw: vec![],
            frozen_system_prompt: None,
            frozen_tools: None,
            frozen_hash: None,
            adaptive_compact: AdaptiveCompactTracker::new(),
            tool_health: crate::engine::tool_health::ToolHealthTracker::new(),
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
