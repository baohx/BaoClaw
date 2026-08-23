use serde_json::Value;

use crate::models::message::{ApiUserMessage, ContentBlock, Message, MessageContent, Usage};
use crate::tools::executor::{ToolExecutionResult, ToolUseRequest};


/// Extract tool use requests from assistant content blocks.
pub fn extract_tool_uses(content_blocks: &[ContentBlock]) -> Vec<ToolUseRequest> {
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
pub fn extract_tool_result_ids(user_message: &ApiUserMessage) -> Vec<String> {
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
pub fn extract_text(content_blocks: &[ContentBlock]) -> Option<String> {
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
pub fn build_tool_result_message(results: &[ToolExecutionResult]) -> Message {
    // Max chars per tool result to avoid exceeding API limits (especially for OpenAI-compatible APIs)
    const MAX_TOOL_RESULT_CHARS: usize = 200_000;

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
pub fn strip_base64_images(value: &Value) -> Value {
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
pub fn accumulate_usage(total: &mut Usage, delta: &Value) {
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
