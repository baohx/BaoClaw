pub use crate::engine::api_builder::*;
pub use crate::engine::query_loop::*;
pub use crate::engine::tool_loop::*;

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
use crate::engine::hooks::{HookManager, TriggerContext, TriggerType};
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
    /// Hook manager for triggering actions on events.
    pub hook_manager: Option<Arc<HookManager>>,
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
    /// System warning (non-fatal error worth surfacing to user, e.g. memory write failure).
    #[serde(rename = "system_warning")]
    SystemWarning {
        message: String,
    },
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
    /// Hook manager for triggering actions on events.
    hook_manager: Option<Arc<HookManager>>,
    /// Intent predictor for context warmup (shared with warmup task).
    intent_predictor: Arc<tokio::sync::Mutex<crate::engine::intent_predictor::IntentPredictor>>,
    /// Warmup manager — preloads resources predicted from user input.
    warmup_manager: Arc<tokio::sync::Mutex<crate::engine::warmup::WarmupManager>>,
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
        
        // Initialize hook manager from config if present
        let hook_manager = config.hook_manager.clone();
        if let Some(ref hm) = hook_manager {
            // Set working directory for the hook executor
            // Note: This is async, but we're in a sync context. The working directory
            // will be set lazily when hooks are first processed.
            let hm_clone = Arc::clone(hm);
            let cwd = config.cwd.clone();
            tokio::spawn(async move {
                hm_clone.set_working_directory(cwd).await;
            });
        }
        
        // Context warmup: intent predictor + warmup manager share the file cache.
        let intent_predictor = Arc::new(tokio::sync::Mutex::new(
            crate::engine::intent_predictor::IntentPredictor::new(),
        ));
        let warmup_manager = Arc::new(tokio::sync::Mutex::new(
            crate::engine::warmup::WarmupManager::new(
                config.cwd.clone(),
                config.file_cache.as_ref().map(Arc::clone),
            ),
        ));
        
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
            hook_manager,
            intent_predictor,
            warmup_manager,
        }
    }

    /// Signal the engine to abort the current operation.
    pub fn abort(&self) {
        let _ = self.abort_tx.send(true);
    }

    /// Shared warmup manager (for hit attribution when files are read,
    /// and for periodic learning passes).
    pub fn warmup_manager_arc(&self) -> Arc<tokio::sync::Mutex<crate::engine::warmup::WarmupManager>> {
        Arc::clone(&self.warmup_manager)
    }

    /// Shared intent predictor (for recording actual intents / accuracy).
    pub fn intent_predictor_arc(&self) -> Arc<tokio::sync::Mutex<crate::engine::intent_predictor::IntentPredictor>> {
        Arc::clone(&self.intent_predictor)
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

    /// Get the session ID (if configured).
    pub fn get_session_id(&self) -> Option<&str> {
        self.config.session_id.as_deref()
    }

    /// Get the working directory.
    pub fn get_cwd(&self) -> &Path {
        &self.config.cwd
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
                Value::String(prompt.clone())
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
            Value::String(prompt.clone())
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
        
        // Trigger UserMessage hook
        if let Some(ref hook_manager) = self.hook_manager {
            let hm = Arc::clone(hook_manager);
            let prompt_text = prompt.clone();
            let cwd = self.config.cwd.clone();
            tokio::spawn(async move {
                let ctx = TriggerContext::user_message(&prompt_text)
                    .with_cwd(cwd);
                let result = hm.process(TriggerType::UserMessage, ctx).await;
                if !result.errors.is_empty() {
                    eprintln!("UserMessage hook errors: {:?}", result.errors);
                }
            });
        }

        // ── Context warmup (Task 4): predict intents and preload resources ──
        // Fully async via tokio::spawn — never blocks the main query flow.
        {
            let predictor = Arc::clone(&self.intent_predictor);
            let warmup = Arc::clone(&self.warmup_manager);
            let prompt_text = prompt.clone();
            tokio::spawn(async move {
                let intents = {
                    let mut p = predictor.lock().await;
                    p.predict_multi(&prompt_text, 3, 0.1)
                };
                let mut w = warmup.lock().await;
                let result = w.warmup(&prompt_text, &intents).await;
                if !result.matched_rules.is_empty() {
                    eprintln!(
                        "Context warmup: rules={:?}, files={}, skills={:?}",
                        result.matched_rules,
                        result.warmed_files.len(),
                        result.preload_skills,
                    );
                }
            });
        }

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
            hook_manager: self.hook_manager.clone(),
            context_window: self.config.context_window,
            auto_compact_threshold_ratio: self.config.auto_compact_threshold_ratio,
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
    /// Hook manager for triggering actions on events.
    pub hook_manager: Option<Arc<HookManager>>,
    /// Model context window (tokens) — propagated to ToolContext for sub-agents.
    pub context_window: u64,
    /// Auto-compact threshold ratio — propagated to ToolContext for sub-agents.
    pub auto_compact_threshold_ratio: f64,
}

impl QueryLoopConfig {
    pub(crate) fn is_aborted(&self) -> bool {
        *self.abort_rx.borrow()
    }
}

/// The core query loop that calls the LLM, processes tool uses, and loops until done.

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
            hook_manager: None,
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
            hook_manager: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
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
            hook_manager: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
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
            hook_manager: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
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
            hook_manager: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
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
            hook_manager: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
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
            hook_manager: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
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
