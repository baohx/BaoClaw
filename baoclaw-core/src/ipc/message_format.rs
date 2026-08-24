//! Message → JSON tail-entry formatting shared by the IPC handlers.
//!
//! `message_to_tail_entry` is the single source of truth for turning an in-memory
//! `Message` into the JSON shape the CLI/TUI/web clients render in history panels.
//! It was previously inlined twice (in the TalkTail and Export handlers), drifting
//! apart; this helper keeps them identical and exposes fine-grained knobs for the
//! richer TalkTail presentation vs the leaner Export one.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::models::message::{ContentBlock, Message, MessageContent};

/// Presentation knobs for `message_to_tail_entry`.
#[derive(Clone, Copy, Default)]
pub struct TailEntryOptions {
    /// Attach `tool_use_id` / `result_output` / `is_error` to user tool-result
    /// entries. Export omits them; TalkTail includes them.
    pub include_tool_result_fields: bool,
    /// Resolve each assistant tool-use id to its result via `tool_results` map
    /// and attach it as `result`. Export omits this; TalkTail includes it.
    pub include_tool_results: bool,
    /// Include `pattern`, `query`, `url`, `prompt` in tool detail lines (TalkTail)
    /// or only `command` + `path` (Export).
    pub include_rich_tool_details: bool,
    /// Attach `duration_ms` and `usage` to assistant entries (TalkTail).
    pub include_assistant_metadata: bool,
}

/// Extract a canonical text payload from a `MessageContent::Content`-style Value
/// (a plain string theater or the Anthropic user block array).
///
/// Non-string, non-array content (e.g. `null`, an object, or a bare number)
/// falls back to its serialized JSON form, matching the pre-refactor handlers.
fn content_text(content: &Value, include_multimodal: bool) -> String {
    match content {
        Value::String(s) => s.clone(),
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
            parts.join(" ")
        }
        other => serde_json::to_string(other).unwrap_or_default(),
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
            let text = content_text(&message.content, opts.include_multimodal_attachments());
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
                        // Match original TalkTail: always emit is_error (true or
                        // false) so clients can distinguish "no error" from "absent".
                        entry["is_error"] = json!(tr.is_error);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::{
        ApiAssistantMessage, ApiUserMessage, ContentBlock, Message, MessageContent,
        ToolUseResult, Usage,
    };
    use std::collections::HashMap;

    /// TalkTail presentation (rich): all knobs on.
    const TALKTAIL: TailEntryOptions = TailEntryOptions {
        include_tool_result_fields: true,
        include_tool_results: true,
        include_rich_tool_details: true,
        include_assistant_metadata: true,
    };
    /// Export presentation (lean): all knobs off.
    const EXPORT: TailEntryOptions = TailEntryOptions {
        include_tool_result_fields: false,
        include_tool_results: false,
        include_rich_tool_details: false,
        include_assistant_metadata: false,
    };

    fn user_msg(content: Value, tool: Option<ToolUseResult>) -> Message {
        Message {
            uuid: "u1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            content: MessageContent::User {
                message: ApiUserMessage { role: "user".into(), content },
                is_meta: false,
                tool_use_result: tool,
            },
        }
    }

    fn assistant_msg(blocks: Vec<ContentBlock>, usage: Option<Usage>) -> Message {
        Message {
            uuid: "a1".into(),
            timestamp: "2026-01-01T00:00:01Z".into(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".into(),
                    content: blocks,
                    stop_reason: None,
                    usage,
                },
                cost_usd: 0.5,
                duration_ms: 420,
            },
        }
    }

    fn tr(id: &str, output: Value, is_error: bool) -> ToolUseResult {
        ToolUseResult { tool_use_id: id.into(), output, is_error }
    }

    #[test]
    fn user_plain_string() {
        let r = message_to_tail_entry(&user_msg(json!("hello"), None), 1, &HashMap::new(), TALKTAIL);
        assert_eq!(r["role"], "user");
        assert_eq!(r["text"], "hello");
        assert!(r.get("is_tool_result").is_none());
        assert_eq!(r["turn"], 1);
    }

    #[test]
    fn user_text_array_with_multimodal() {
        let content = json!([
            {"type":"text","text":"see this:"},
            {"type":"image","source":{}},
            {"type":"document","source":{}}
        ]);
        // rich (TalkTail) keeps markers
        let rich = message_to_tail_entry(&user_msg(content.clone(), None), 1, &HashMap::new(), TALKTAIL);
        assert!(rich["text"].as_str().unwrap().contains("[image]"));
        assert!(rich["text"].as_str().unwrap().contains("[document]"));
        // lean (Export) skips them
        let lean = message_to_tail_entry(&user_msg(content, None), 1, &HashMap::new(), EXPORT);
        assert_eq!(lean["text"], "see this:");
    }

    #[test]
    fn user_tool_result_is_error_true() {
        let m = user_msg(json!("op done"), Some(tr("t1", json!({"ok":true}), true)));
        let r = message_to_tail_entry(&m, 1, &HashMap::new(), TALKTAIL);
        assert_eq!(r["is_tool_result"], true);
        assert_eq!(r["tool_use_id"], "t1");
        assert_eq!(r["result_output"], json!({"ok":true}));
        assert_eq!(r["is_error"], true);
    }

    #[test]
    fn user_tool_result_is_error_false_present_for_talktail() {
        // Guards the T2 regression: is_error must be present even when false.
        let m = user_msg(json!("ok"), Some(tr("t2", json!(null), false)));
        let r = message_to_tail_entry(&m, 1, &HashMap::new(), TALKTAIL);
        assert_eq!(r["is_tool_result"], true);
        assert_eq!(r["is_error"], false, "is_error must be present and false for TalkTail");
    }

    #[test]
    fn user_tool_result_export_omits_detail_fields() {
        let m = user_msg(json!("ok"), Some(tr("t1", json!({"ok":true}), true)));
        let r = message_to_tail_entry(&m, 1, &HashMap::new(), EXPORT);
        assert_eq!(r["is_tool_result"], true);
        assert!(r.get("tool_use_id").is_none(), "Export must not emit tool_use_id");
        assert!(r.get("result_output").is_none(), "Export must not emit result_output");
        assert!(r.get("is_error").is_none(), "Export must not emit is_error");
    }

    #[test]
    fn user_unknown_content_serializes_to_json() {
        // Guards the T1 regression: unknown content falls back to its JSON text.
        let m = user_msg(Value::Null, None);
        let r = message_to_tail_entry(&m, 1, &HashMap::new(), TALKTAIL);
        assert_eq!(r["text"], "null");
    }

    #[test]
    fn user_object_content_serializes_to_json() {
        let m = user_msg(json!({"hello":"world"}), None);
        let r = message_to_tail_entry(&m, 1, &HashMap::new(), TALKTAIL);
        assert_eq!(r["text"], json!({"hello":"world"}).to_string());
    }

    #[test]
    fn assistant_text_and_tool_result_attachment() {
        let tool = ContentBlock::ToolUse {
            id: "tu1".into(),
            name: "bash".into(),
            input: json!({"command":"ls"}),
        };
        let blocks = vec![ContentBlock::Text { text: "done".into() }, tool];
        let mut results = HashMap::new();
        results.insert("tu1".into(), json!("file listing"));
        // TalkTail attaches result
        let rich = message_to_tail_entry(&assistant_msg(blocks.clone(), None), 2, &results, TALKTAIL);
        assert_eq!(rich["text"], "done");
        assert_eq!(rich["tools"][0]["name"], "bash");
        assert_eq!(rich["tools"][0]["result"], "file listing");
        assert_eq!(rich["tools"][0]["detail"], "command: ls");
        assert_eq!(rich["cost_usd"], 0.5);
        assert_eq!(rich["duration_ms"], 420);
        // Export omits result and metadata
        let lean = message_to_tail_entry(&assistant_msg(blocks, None), 2, &results, EXPORT);
        assert!(lean["tools"][0].get("result").is_none());
        assert!(lean.get("duration_ms").is_none());
        assert!(lean.get("usage").is_none());
        assert_eq!(lean["cost_usd"], 0.5); // cost_usd still present
    }

    #[test]
    fn assistant_rich_tool_details_all_fields() {
        // Covers the rich-detail branches (pattern/query/url/prompt + prompt
        // truncation) that the export/lean test deliberately omits.
        let tool = ContentBlock::ToolUse {
            id: "tu2".into(),
            name: "grep".into(),
            input: json!({
                "command": "rg",
                "file_path": "src/main.rs",
                "pattern": "TODO",
                "query": "config",
                "url": "https://example.com",
                "prompt": "x".repeat(300),
            }),
        };
        let blocks = vec![tool];
        let rich = message_to_tail_entry(&assistant_msg(blocks.clone(), None), 2, &HashMap::new(), TALKTAIL);
        let rich_tool = &rich["tools"][0];
        // 300-char prompt is truncated to 200; build the expected 200-char suffix.
        let expected_prompt = "prompt: ".to_string() + &"x".repeat(200);
        let expected_detail = format!("command: rg, path: src/main.rs, pattern: TODO, query: config, url: https://example.com, {}", expected_prompt);
        assert_eq!(rich_tool["detail"], expected_detail);
        // prompt truncated to 200 chars
        let prompt_part = expected_prompt.strip_prefix("prompt: ").unwrap();
        assert_eq!(prompt_part.len(), 200, "prompt must be truncated to 200 chars");

        // Export (lean) must drop the rich fields, keeping only command + path
        let lean = message_to_tail_entry(&assistant_msg(blocks, None), 2, &HashMap::new(), EXPORT);
        assert_eq!(lean["tools"][0]["detail"], "command: rg, path: src/main.rs");
        assert!(!lean["tools"][0]["detail"].as_str().unwrap().contains("pattern:"));
        assert!(!lean["tools"][0]["detail"].as_str().unwrap().contains("prompt:"));
    }

    #[test]
    fn assistant_metadata_usage_attached_when_requested() {
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: Some(2),
        };
        let m = assistant_msg(vec![ContentBlock::Text { text: "hi".into() }], Some(usage));
        let rich = message_to_tail_entry(&m, 1, &HashMap::new(), TALKTAIL);
        assert_eq!(rich["usage"]["input_tokens"], 10);
        assert_eq!(rich["usage"]["cache_read_input_tokens"], 2);
        let lean = message_to_tail_entry(&m, 1, &HashMap::new(), EXPORT);
        assert!(lean.get("usage").is_none());
    }
}
