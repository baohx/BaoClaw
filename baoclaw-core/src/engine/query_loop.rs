use futures::StreamExt;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use crate::api::client::{ApiError, ApiStreamEvent, CreateMessageRequest};
use crate::api::fallback::{FallbackAction, FallbackController};
use crate::api::unified::UnifiedClient;
use crate::config::BaoclawConfig;
use crate::engine::api_builder::build_api_request;
use crate::engine::cost_tracker::CostTracker;
use crate::engine::git_info::{get_git_info_async, GitInfo};
use crate::engine::hooks::{HookManager, TriggerContext, TriggerType};
use crate::engine::session_memory::SessionMemory;
use crate::engine::token_counter::BudgetStatus;
use crate::engine::transcript::{TranscriptEntry, TranscriptEntryType, TranscriptWriter};
use crate::models::message::{
    ApiAssistantMessage, ApiUserMessage, ContentBlock, Message, MessageContent, Usage,
};
use crate::tools::executor::{execute_tools, ToolExecutionResult, ToolUseRequest};
use crate::tools::trait_def::{ProgressSender, ToolContext};

use crate::engine::query_engine::{
    estimate_tokens, estimate_tokens_str, format_messages_for_summary, AdaptiveCompactTracker, CompactResult, EngineError, EngineEvent,
    NoopProgressSender, QueryLoopConfig, QueryResult, QueryStatus, EMPTY_USAGE,
};
use crate::engine::tool_loop::{
    accumulate_usage, build_tool_result_message, extract_text, extract_tool_result_ids, extract_tool_uses,
};

pub async fn run_query_loop(
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
        context_window: config.context_window,
        auto_compact_threshold_ratio: config.auto_compact_threshold_ratio,
        tool_output_threshold_chars: crate::config::default_tool_output_threshold_chars(),
        model_profiles: std::collections::HashMap::new(),
        primary_profile: None,
        fallback_profiles: Vec::new(),
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
            hook_manager: config.hook_manager.clone(),
            context_window: config.context_window,
            auto_compact_threshold_ratio: config.auto_compact_threshold_ratio,
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

        // Trigger AssistantMessage hook
        if let Some(ref hook_manager) = config.hook_manager {
            let hm = Arc::clone(hook_manager);
            let text: String = assistant_content_blocks.iter().filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            }).collect::<Vec<_>>().join(" ");
            let cwd = config.cwd.clone();
            tokio::spawn(async move {
                let ctx = TriggerContext::assistant_message(&text)
                    .with_cwd(cwd);
                let result = hm.process(TriggerType::AssistantMessage, ctx).await;
                if !result.errors.is_empty() {
                    eprintln!("AssistantMessage hook errors: {:?}", result.errors);
                }
            });
        }

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
                    let _compact_model = config.model.clone();
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
            context_window: config.context_window,
            auto_compact_threshold_ratio: config.auto_compact_threshold_ratio,
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

            // Trigger ToolResult hook for each tool result
            if let Some(ref hook_manager) = config.hook_manager {
                // Find the tool use for this result
                let tool_use = tool_uses.iter().find(|tu| tu.id == result.tool_use_id);
                let tool_name = tool_use.map(|tu| tu.name.clone()).unwrap_or_default();
                let input = tool_use.map(|tu| serde_json::to_string(&tu.input).unwrap_or_default()).unwrap_or_default();
                let output = serde_json::to_string(&result.output).unwrap_or_default();

                let hm = Arc::clone(hook_manager);
                let cwd = config.cwd.clone();
                tokio::spawn(async move {
                    let ctx = TriggerContext::tool_result(&tool_name, &input, &output)
                        .with_cwd(cwd);
                    let result = hm.process(TriggerType::ToolResult, ctx).await;
                    if !result.errors.is_empty() {
                        eprintln!("ToolResult hook errors: {:?}", result.errors);
                    }
                });

                // Trigger file-related hooks for file operation tools
                // Only trigger on successful file operations (not errors)
                if !result.is_error {
                    if let Some(tool_use) = tool_use {
                        let trigger_type = match tool_use.name.as_str() {
                            "Write" | "write_file" | "FileWrite" => {
                                tool_use.input.get("file_path")
                                    .or_else(|| tool_use.input.get("path"))
                                    .and_then(|v| v.as_str())
                                    .map(|path| (TriggerType::FileCreated, path.to_string()))
                            }
                            "Edit" | "edit_file" | "FileEdit" => {
                                tool_use.input.get("file_path")
                                    .or_else(|| tool_use.input.get("path"))
                                    .and_then(|v| v.as_str())
                                    .map(|path| (TriggerType::FileEdited, path.to_string()))
                            }
                            "Delete" | "delete_file" | "FileDelete" => {
                                tool_use.input.get("file_path")
                                    .or_else(|| tool_use.input.get("path"))
                                    .and_then(|v| v.as_str())
                                    .map(|path| (TriggerType::FileDeleted, path.to_string()))
                            }
                            _ => None,
                        };

                        if let Some((trigger, file_path)) = trigger_type {
                            let hm = Arc::clone(hook_manager);
                            let cwd = config.cwd.clone();
                            tokio::spawn(async move {
                                let ctx = match trigger {
                                    TriggerType::FileCreated => TriggerContext::file_created(&file_path, &cwd),
                                    TriggerType::FileEdited => TriggerContext::file_edited(&file_path, &cwd),
                                    TriggerType::FileDeleted => TriggerContext::file_deleted(&file_path, &cwd),
                                    _ => TriggerContext::new(),
                                };
                                let result = hm.process(trigger, ctx).await;
                                if !result.errors.is_empty() {
                                    eprintln!("File hook errors: {:?}", result.errors);
                                }
                            });
                        }
                    }
                }
            }
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

/// Compact messages in-place: summarize old messages via API and replace with a boundary.
/// Used both for preemptive auto-compaction and for recovery after context-overflow errors.
pub async fn compact_messages(
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
pub fn micro_compact(messages: &mut Vec<Message>, idle_threshold_secs: u64) {
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
pub fn session_memory_compact(
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
pub fn reactive_compact(messages: &mut Vec<Message>, target_reduction: Option<usize>) {
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
pub fn validate_and_fix_tool_messages(messages: &[Message]) -> Vec<Message> {
    eprintln!("=== validate_and_fix_tool_messages: START ===");
    eprintln!("  Input messages: {}", messages.len());

    // First pass: collect all tool_use IDs and their corresponding tool_result IDs
    let mut tool_use_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut tool_result_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (_idx, msg) in messages.iter().enumerate() {
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
/// Background task: generate a session summary and persist it.
///
/// Spawned (fire-and-forget) after each query loop iteration when
/// `session_memory.should_update()` returns true.  Uses a lightweight
/// API call (no tools, no conversation history) to generate a rolling
/// summary that is loaded instantly on next session startup.
pub async fn update_session_memory_background(
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
