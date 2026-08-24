//! Message → JSON tail-entry formatting shared by the IPC handlers.
//!
//! `message_to_tail_entry` is the single source of truth for turning an in-memory
//! `Message` into the JSON shape the CLI/TUI/web clients render in history panels.
//! It was previously inlined twice (TalkTail and SearchHistory), drifting apart;
//! this helper keeps them identical and exposes fine-grained knobs for the richer
//! TalkTail presentation vs the leaner SearchHistory one.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::models::message::{ContentBlock, Message, MessageContent};

/// Presentation knobs for `message_to_tail_entry`.
#[derive(Clone, Copy, Default)]
pub struct TailEntryOptions {
    /// Attach `tool_use_id` / `result_output` / `is_error` to user tool-result
    /// entries. SearchHistory omits them; TalkTail includes them.
    pub include_tool_result_fields: bool,
    /// Resolve each assistant tool-use id to its result via `tool_results` map
    /// and attach it as `result`. SearchHistory omits this; TalkTail includes it.
    pub include_tool_results: bool,
    /// Include `pattern`, `query`, `url`, `prompt` in tool detail lines (TalkTail)
    /// or only `command` + `path` (SearchHistory).
    pub include_rich_tool_details: bool,
    /// Attach `duration_ms` and `usage` to assistant entries (TalkTail).
    pub include_assistant_metadata: bool,
}

/// Extract a canonical text payload from a `MessageContent::Content`-style Value
/// (a plain string theater or the Anthropic user block array).
fn content_text(content: &Value, include_multimodal: bool) -> Result<String, String> {
    match content {
        Value::String(s) => Ok(s.clone()),
        Value::Array(arr) => {
            let mut parts: Vec<String> = Vec::new();
            for b in arr {
                let ty = b.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                let text = b.get("text").and_then(|t| t.as_str());
                match ty {
                    "text" => {
                        if let Some(t) = text {
                            parts.push(t.to_string());
                        }
                    }
                    "image" | "document" if include_multimodal => {
                        parts.push(format!("[{}]", ty));
                    }
                    "image" | "document" => { /* skipped */ }
                    _ => {
                        if let Some(t) = text {
                            parts.push(t.to_string());
                        }
                    }
                }
            }
            Ok(parts.join(" "))
        }
        _ => Err(serde_json::to_string(content).unwrap_or_default()),
    }
}

/// Format a single message into the shared history-entry JSON shape.
/// `tool_results` maps tool_use_id → output Value (built by the caller from the
/// session's user messages), used when `include_tool_results` is set.
pub fn message_to_tail_entry(
    m: &Message,
    turn_num: usize,
    tool_results: &HashMap<String, Value>,
    opts: TailEntryOptions,
) -> Value {
    match &m.content {
        MessageContent::User { message, tool_use_result, .. } => {
            let text = content_text(&message.content, opts.include_multimodal_attachments())
                .unwrap_or_default();
            let is_tool_result = tool_use_result.is_some();
            let mut entry = json!({
                "role": "user",
                "text": text,
                "timestamp": m.timestamp,
                "turn": turn_num,
            });
            if is_tool_result {
                entry["is_tool_result"] = json!(true);
                if opts.include_tool_result_fields {
                    if let Some(tr) = tool_use_result {
                        entry["tool_use_id"] = json!(tr.tool_use_id);
                        entry["result_output"] = tr.output.clone();
                        if tr.is_error {
                            entry["is_error"] = json!(true);
                        }
                    }
                }
            }
            entry
        }
        MessageContent::Assistant { message, cost_usd, duration_ms, .. } => {
            let text: String = message.content.iter().filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            }).collect::<Vec<_>>().join("");

            let tools: Vec<Value> = message.content.iter().filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => {
                    let mut info = json!({"name": name, "id": id});
                    let mut details: Vec<String> = Vec::new();
                    if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                        details.push(format!("command: {}", cmd));
                    }
                    if let Some(fp) = input.get("file_path").and_then(|v| v.as_str()) {
                        details.push(format!("path: {}", fp));
                    }
                    if opts.include_rich_tool_details {
                        if let Some(p) = input.get("pattern").and_then(|v| v.as_str()) {
                            details.push(format!("pattern: {}", p));
                        }
                        if let Some(q) = input.get("query").and_then(|v| v.as_str()) {
                            details.push(format!("query: {}", q));
                        }
                        if let Some(u) = input.get("url").and_then(|v| v.as_str()) {
                            details.push(format!("url: {}", u));
                        }
                        if let Some(p) = input.get("prompt").and_then(|v| v.as_str()) {
                            details.push(format!("prompt: {}", p.chars().take(200).collect::<String>()));
                        }
                    }
                    if !details.is_empty() {
                        info["detail"] = json!(details.join(", "));
                    }
                    if opts.include_tool_results {
                        if let Some(result) = tool_results.get(id) {
                            info["result"] = result.clone();
                        }
                    }
                    Some(info)
                }
                _ => None,
            }).collect();

            let mut entry = json!({
                "role": "assistant",
                "text": text,
                "timestamp": m.timestamp,
                "turn": turn_num,
                "cost_usd": cost_usd,
            });
            if opts.include_assistant_metadata {
                entry["duration_ms"] = json!(duration_ms);
                if let Some(usage) = &message.usage {
                    entry["usage"] = json!(usage);
                }
            }
            if !tools.is_empty() {
                entry["tools"] = json!(tools);
            }
            entry
        }
        _ => json!({
            "role": "system",
            "text": "",
            "timestamp": m.timestamp,
            "turn": turn_num,
        }),
    }
}

impl TailEntryOptions {
    /// Multimodal attachments (image/document markers) are only surfaced in the
    /// rich TalkTail view; the lean view keeps plain text only.
    fn include_multimodal_attachments(&self) -> bool {
        self.include_rich_tool_details
    }
}
