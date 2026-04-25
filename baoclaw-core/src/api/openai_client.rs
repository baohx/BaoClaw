/// OpenAI-compatible API client.
///
/// Translates BaoClaw's Anthropic-format requests into OpenAI chat completion
/// requests, and translates OpenAI SSE responses back into ApiStreamEvent
/// so that QueryEngine works without changes.

use bytes::Bytes;
use futures::stream::Stream;
use serde_json::{json, Value};
use std::pin::Pin;
use std::task::{Context, Poll};

use super::client::{ApiClientConfig, ApiError, ApiStreamEvent, CreateMessageRequest};

const DEFAULT_OPENAI_URL: &str = "https://api.openai.com";

/// OpenAI-compatible API client.
pub struct OpenAiClient {
    http_client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenAiClient {
    pub fn new(config: ApiClientConfig) -> Self {
        let http_client = reqwest::Client::builder()
            .build()
            .expect("Failed to build HTTP client");
        Self {
            http_client,
            api_key: config.api_key,
            base_url: config.base_url.unwrap_or_else(|| DEFAULT_OPENAI_URL.to_string()),
        }
    }

    /// Convert Anthropic-format request to OpenAI chat completion request.
    fn convert_request(&self, req: &CreateMessageRequest) -> Value {
        let mut messages: Vec<Value> = Vec::new();

        // System prompt
        if let Some(system_blocks) = &req.system {
            let system_text: String = system_blocks.iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n\n");
            if !system_text.is_empty() {
                messages.push(json!({"role": "system", "content": system_text}));
            }
        }

        // Conversation messages
        for msg in &req.messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = &msg["content"];

            match role {
                "user" => {
                    // Handle content blocks (may include text, tool_result, image, document)
                    if let Some(arr) = content.as_array() {
                        // Collect multimodal parts for a single user message
                        let mut parts: Vec<Value> = Vec::new();

                        for block in arr {
                            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match block_type {
                                "tool_result" => {
                                    // Tool results become separate "tool" role messages
                                    // Flush any accumulated parts first
                                    if !parts.is_empty() {
                                        messages.push(json!({"role": "user", "content": Value::Array(parts.clone())}));
                                        parts.clear();
                                    }
                                    let tool_call_id = block.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                                    let output = block.get("content").cloned().unwrap_or(Value::Null);
                                    let output_str = if output.is_string() {
                                        output.as_str().unwrap_or("").to_string()
                                    } else {
                                        serde_json::to_string(&output).unwrap_or_default()
                                    };
                                    messages.push(json!({
                                        "role": "tool",
                                        "tool_call_id": tool_call_id,
                                        "content": output_str,
                                    }));
                                }
                                "text" => {
                                    let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                    parts.push(json!({"type": "text", "text": text}));
                                }
                                "image" => {
                                    // Convert Anthropic image format to OpenAI image_url format
                                    if let Some(source) = block.get("source") {
                                        let media_type = source.get("media_type").and_then(|v| v.as_str()).unwrap_or("image/png");
                                        let data = source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                        let data_url = format!("data:{};base64,{}", media_type, data);
                                        parts.push(json!({
                                            "type": "image_url",
                                            "image_url": {"url": data_url}
                                        }));
                                    }
                                }
                                "document" => {
                                    // OpenAI doesn't natively support document blocks;
                                    // include as text description noting the document was attached
                                    if let Some(source) = block.get("source") {
                                        let media_type = source.get("media_type").and_then(|v| v.as_str()).unwrap_or("application/octet-stream");
                                        // For models that support it, pass as image_url with data URI
                                        // Otherwise fall back to a text note
                                        let data = source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                        if media_type == "application/pdf" {
                                            // Some OpenAI-compatible APIs support PDF via file content
                                            let data_url = format!("data:{};base64,{}", media_type, data);
                                            parts.push(json!({
                                                "type": "image_url",
                                                "image_url": {"url": data_url}
                                            }));
                                        } else {
                                            parts.push(json!({
                                                "type": "text",
                                                "text": format!("[Attached document: {}]", media_type)
                                            }));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        // Flush remaining parts
                        if !parts.is_empty() {
                            if parts.len() == 1 && parts[0].get("type").and_then(|t| t.as_str()) == Some("text") {
                                // Single text part: send as plain string for compatibility
                                messages.push(json!({"role": "user", "content": parts[0]["text"].clone()}));
                            } else {
                                messages.push(json!({"role": "user", "content": Value::Array(parts)}));
                            }
                        }
                        if arr.is_empty() {
                            messages.push(json!({"role": "user", "content": ""}));
                        }
                    } else {
                        // Plain string content
                        messages.push(json!({"role": "user", "content": content}));
                    }
                }
                "assistant" => {
                    // Convert assistant content blocks to OpenAI format
                    if let Some(arr) = content.as_array() {
                        let mut text_parts = Vec::new();
                        let mut tool_calls = Vec::new();
                        let mut reasoning_parts = Vec::new();

                        for block in arr {
                            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match block_type {
                                "text" => {
                                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                        text_parts.push(t.to_string());
                                    }
                                }
                                "tool_use" => {
                                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                    let input = block.get("input").cloned().unwrap_or(json!({}));
                                    tool_calls.push(json!({
                                        "id": id,
                                        "type": "function",
                                        "function": {
                                            "name": name,
                                            "arguments": serde_json::to_string(&input).unwrap_or_default(),
                                        }
                                    }));
                                }
                                "thinking" => {
                                    if let Some(t) = block.get("thinking").and_then(|v| v.as_str()) {
                                        reasoning_parts.push(t.to_string());
                                    }
                                }
                                _ => {}
                            }
                        }

                        let mut msg = json!({"role": "assistant"});
                        let text = text_parts.join("");
                        if !text.is_empty() || tool_calls.is_empty() {
                            msg["content"] = Value::String(text);
                        }
                        if !tool_calls.is_empty() {
                            msg["tool_calls"] = Value::Array(tool_calls);
                        }
                        // DeepSeek requires reasoning_content to be passed back on all assistant messages
                        // when thinking mode is active. Include it even if empty.
                        let reasoning = reasoning_parts.join("");
                        msg["reasoning_content"] = Value::String(reasoning);
                        messages.push(msg);
                    } else {
                        messages.push(json!({"role": "assistant", "content": content}));
                    }
                }
                _ => {
                    // Convert mid-conversation system messages (e.g. CompactBoundary)
                    // to user messages, since many OpenAI-compatible APIs (GLM, etc.)
                    // only allow system role at the start of the conversation.
                    if role == "system" {
                        let text = match content {
                            Value::String(s) => s.clone(),
                            _ => serde_json::to_string(content).unwrap_or_default(),
                        };
                        messages.push(json!({"role": "user", "content": format!("[System context]\n{}", text)}));
                    } else {
                        messages.push(json!({"role": role, "content": content}));
                    }
                }
            }
        }

        // Tools
        let tools: Option<Vec<Value>> = req.tools.as_ref().map(|tools| {
            tools.iter().map(|t| {
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                let desc = t.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let schema = t.get("input_schema").cloned().unwrap_or(json!({"type": "object"}));
                json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": desc,
                        "parameters": schema,
                    }
                })
            }).collect()
        });

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_tokens,
            "stream": true,
            "stream_options": {"include_usage": true},
        });

        if let Some(tools) = tools {
            if !tools.is_empty() {
                body["tools"] = Value::Array(tools);
            }
        }

        body
    }

    /// Send a streaming chat completion request and return an ApiStreamEvent stream.
    /// Translates OpenAI SSE format to Anthropic ApiStreamEvent format.
    pub async fn create_message_stream(
        &self,
        request: CreateMessageRequest,
    ) -> Result<OpenAiSseStream, ApiError> {
        let body = self.convert_request(&request);
        let url = if self.base_url.contains("/chat/completions") {
            self.base_url.clone()
        } else if self.base_url.ends_with("/v1") || self.base_url.ends_with("/v4") {
            format!("{}/chat/completions", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        };

        let response = self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| ApiError::NetworkError(e.to_string()))?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let message = response.text().await.unwrap_or_default();
            if status == 400 {
                // Dump the request for debugging message format issues
                eprintln!("=== API 400 Bad Request ===");
                if let Some(msgs) = body.get("messages") {
                    if let Some(arr) = msgs.as_array() {
                        for (i, m) in arr.iter().enumerate() {
                            let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("?");
                            let content = m.get("content").map(|c| {
                                let s = c.to_string();
                                if s.len() > 200 {
                                    // Safe UTF-8 truncation
                                    let truncated: String = s.chars().take(200).collect();
                                    format!("{}...[{}chars]", truncated, s.len())
                                } else { s }
                            }).unwrap_or_default();
                            let tc = m.get("tool_calls").map(|t| format!(" tool_calls:{}", t.to_string().len())).unwrap_or_default();
                            let tid = m.get("tool_call_id").and_then(|t| t.as_str()).map(|s| format!(" tool_call_id:{}", s)).unwrap_or_default();
                            eprintln!("  [{}] role={}{}{} content={}", i, role, tc, tid, content);
                        }
                    }
                }
                eprintln!("=== Response: {} ===", message);
            }
            return Err(match status {
                401 => ApiError::AuthError,
                429 => ApiError::RateLimited,
                400 => ApiError::BadRequest { message },
                500..=599 => ApiError::ServerError { status },
                _ => ApiError::HttpError { status, message },
            });
        }

        Ok(OpenAiSseStream::new(response.bytes_stream()))
    }
}

/// SSE stream that translates OpenAI chat completion chunks to ApiStreamEvent.
pub struct OpenAiSseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    buffer: String,
    sent_message_start: bool,
    content_index: u32,
    tool_call_states: Vec<ToolCallState>,
    finished: bool,
    pending_events: Vec<Result<ApiStreamEvent, ApiError>>,
    in_thinking: bool,
    text_started: bool,
    text_block_index: u32,
}

struct ToolCallState {
    index: u32,
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

impl OpenAiSseStream {
    fn new(stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(stream),
            buffer: String::new(),
            sent_message_start: false,
            content_index: 0,
            tool_call_states: Vec::new(),
            finished: false,
            pending_events: Vec::new(),
            in_thinking: false,
            text_started: false,
            text_block_index: 0,
        }
    }

    /// Parse one OpenAI SSE chunk and produce zero or more ApiStreamEvents.
    fn translate_chunk(&mut self, data: &str) -> Vec<Result<ApiStreamEvent, ApiError>> {
        let mut events = Vec::new();

        if data == "[DONE]" {
            // Emit content_block_stop for any open blocks, then message_stop
            for tc in &self.tool_call_states {
                if tc.started {
                    events.push(Ok(ApiStreamEvent::ContentBlockStop { index: tc.index }));
                }
            }
            events.push(Ok(ApiStreamEvent::MessageStop));
            self.finished = true;
            return events;
        }

        let chunk: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                events.push(Err(ApiError::ParseError(format!("Invalid JSON: {}", e))));
                return events;
            }
        };

        // Emit message_start on first chunk
        if !self.sent_message_start {
            self.sent_message_start = true;
            let usage = chunk.get("usage").cloned().unwrap_or(json!({}));
            events.push(Ok(ApiStreamEvent::MessageStart {
                message: json!({
                    "id": chunk.get("id").cloned().unwrap_or(Value::Null),
                    "model": chunk.get("model").cloned().unwrap_or(Value::Null),
                    "usage": {
                        "input_tokens": usage.get("prompt_tokens").cloned().unwrap_or(json!(0)),
                        "output_tokens": json!(0),
                    }
                }),
            }));
        }

        let choices = chunk.get("choices").and_then(|c| c.as_array());
        if let Some(choices) = choices {
            for choice in choices {
                let delta = &choice["delta"];
                let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

                // Text content
                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                    if !content.is_empty() {
                        // If we were in thinking mode, close thinking block first
                        if self.in_thinking {
                            events.push(Ok(ApiStreamEvent::ContentBlockStop { index: 0 }));
                            self.in_thinking = false;
                            self.content_index = 1;
                        }
                        // Emit content_block_start on first text
                        if !self.text_started && self.tool_call_states.is_empty() {
                            events.push(Ok(ApiStreamEvent::ContentBlockStart {
                                index: self.content_index,
                                content_block: json!({"type": "text", "text": ""}),
                            }));
                            self.text_started = true;
                            self.text_block_index = self.content_index;
                            self.content_index += 1;
                        }
                        events.push(Ok(ApiStreamEvent::ContentBlockDelta {
                            index: self.text_block_index,
                            delta: json!({"type": "text_delta", "text": content}),
                        }));
                    }
                }

                // DeepSeek reasoning_content (thinking/chain-of-thought)
                if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                    if !reasoning.is_empty() {
                        if !self.in_thinking {
                            events.push(Ok(ApiStreamEvent::ContentBlockStart {
                                index: 0,
                                content_block: json!({"type": "thinking", "thinking": ""}),
                            }));
                            self.in_thinking = true;
                        }
                        events.push(Ok(ApiStreamEvent::ContentBlockDelta {
                            index: 0,
                            delta: json!({"type": "thinking_delta", "thinking": reasoning}),
                        }));
                    }
                }

                // Tool calls
                if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
                    for tc in tool_calls {
                        let tc_index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                        let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let func = &tc["function"];
                        let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let args_chunk = func.get("arguments").and_then(|v| v.as_str()).unwrap_or("");

                        // Ensure we have a state for this tool call
                        while self.tool_call_states.len() <= tc_index {
                            let idx = self.content_index + self.tool_call_states.len() as u32;
                            self.tool_call_states.push(ToolCallState {
                                index: idx,
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                                started: false,
                            });
                        }

                        let state = &mut self.tool_call_states[tc_index];
                        if !tc_id.is_empty() { state.id = tc_id; }
                        if !name.is_empty() { state.name = name; }
                        state.arguments.push_str(args_chunk);

                        // Emit content_block_start on first chunk for this tool call
                        if !state.started {
                            // Close thinking block if open
                            if self.in_thinking {
                                events.push(Ok(ApiStreamEvent::ContentBlockStop { index: 0 }));
                                self.in_thinking = false;
                            }
                            // Close text block if open
                            if self.text_started && tc_index == 0 {
                                events.push(Ok(ApiStreamEvent::ContentBlockStop { index: self.text_block_index }));
                                self.text_started = false;
                            } else if self.content_index == 1 && tc_index == 0 {
                                events.push(Ok(ApiStreamEvent::ContentBlockStop { index: 0 }));
                            }
                            state.started = true;
                            events.push(Ok(ApiStreamEvent::ContentBlockStart {
                                index: state.index,
                                content_block: json!({
                                    "type": "tool_use",
                                    "id": state.id,
                                    "name": state.name,
                                    "input": {},
                                }),
                            }));
                        }

                        // Emit argument delta
                        if !args_chunk.is_empty() {
                            events.push(Ok(ApiStreamEvent::ContentBlockDelta {
                                index: state.index,
                                delta: json!({"type": "input_json_delta", "partial_json": args_chunk}),
                            }));
                        }
                    }
                }

                // Finish reason
                if let Some(reason) = finish_reason {
                    // Close open thinking block
                    if self.in_thinking {
                        events.push(Ok(ApiStreamEvent::ContentBlockStop { index: 0 }));
                        self.in_thinking = false;
                    }
                    // Close open text block
                    if self.text_started {
                        events.push(Ok(ApiStreamEvent::ContentBlockStop { index: self.text_block_index }));
                    } else if self.content_index == 1 && self.tool_call_states.is_empty() {
                        events.push(Ok(ApiStreamEvent::ContentBlockStop { index: 0 }));
                    }

                    let stop_reason = match reason {
                        "stop" => "end_turn",
                        "tool_calls" => "tool_use",
                        "length" => "max_tokens",
                        "content_filter" => "end_turn",
                        other => other,
                    };

                    // Usage from the final chunk
                    let usage = chunk.get("usage").cloned().unwrap_or(json!({}));
                    events.push(Ok(ApiStreamEvent::MessageDelta {
                        delta: json!({"stop_reason": stop_reason}),
                        usage: json!({
                            "input_tokens": usage.get("prompt_tokens").cloned().unwrap_or(json!(0)),
                            "output_tokens": usage.get("completion_tokens").cloned().unwrap_or(json!(0)),
                        }),
                    }));
                }
            }
        }

        events
    }
}

impl Stream for OpenAiSseStream {
    type Item = Result<ApiStreamEvent, ApiError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }

        // Check buffer for complete SSE events
        loop {
            if let Some(event) = self.parse_next_sse() {
                return Poll::Ready(Some(event));
            }

            // Need more data
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    let text = String::from_utf8_lossy(&bytes);
                    self.buffer.push_str(&text);
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(ApiError::NetworkError(e.to_string()))));
                }
                Poll::Ready(None) => {
                    if !self.finished {
                        self.finished = true;
                        return Poll::Ready(Some(Ok(ApiStreamEvent::MessageStop)));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl OpenAiSseStream {
    fn parse_next_sse(&mut self) -> Option<Result<ApiStreamEvent, ApiError>> {
        // Return pending events first
        if !self.pending_events.is_empty() {
            return Some(self.pending_events.remove(0));
        }

        // OpenAI SSE format: "data: {...}\n\n"
        loop {
            let newline_pos = self.buffer.find("\n\n")?;
            let chunk = self.buffer[..newline_pos].to_string();
            self.buffer = self.buffer[newline_pos + 2..].to_string();

            for line in chunk.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with(':') { continue; }

                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();
                    if data.is_empty() { continue; }

                    let mut translated = self.translate_chunk(data);
                    if !translated.is_empty() {
                        let first = translated.remove(0);
                        // Queue remaining events for subsequent polls
                        self.pending_events.extend(translated);
                        return Some(first);
                    }
                }
            }
        }
    }
}
