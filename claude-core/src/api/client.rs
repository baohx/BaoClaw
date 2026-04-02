use bytes::Bytes;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::task::{Context, Poll};

// --- Configuration ---

/// Configuration for creating an AnthropicClient.
pub struct ApiClientConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub max_retries: Option<u32>,
}

/// The Anthropic API client for streaming message creation.
pub struct AnthropicClient {
    http_client: reqwest::Client,
    api_key: String,
    base_url: String,
    max_retries: u32,
}

// --- Request types ---

/// Request body for the Anthropic Messages API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    pub max_tokens: u32,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// --- SSE stream event types ---

/// SSE stream events from the Anthropic API.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ApiStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: serde_json::Value },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: serde_json::Value,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: u32,
        delta: serde_json::Value,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: serde_json::Value,
        usage: serde_json::Value,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "error")]
    Error { error: ApiErrorDetail },
}

/// Detail of an API error returned in the SSE stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiErrorDetail {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

// --- Error types ---

/// Errors that can occur when communicating with the Anthropic API.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP error: status {status}, message: {message}")]
    HttpError { status: u16, message: String },
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Rate limited (429)")]
    RateLimited,
    #[error("Server error ({status})")]
    ServerError { status: u16 },
    #[error("Auth error (401)")]
    AuthError,
    #[error("Bad request (400): {message}")]
    BadRequest { message: String },
}

impl ApiError {
    /// Returns true if this error is retryable (rate limited or server error).
    pub fn is_retryable(&self) -> bool {
        matches!(self, ApiError::RateLimited | ApiError::ServerError { .. })
    }

    /// Returns true if this is an authentication error.
    pub fn is_auth_error(&self) -> bool {
        matches!(self, ApiError::AuthError)
    }
}

// --- SSE Stream wrapper ---

/// An async stream that parses SSE events from a reqwest response body.
pub struct SseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    buffer: String,
}

impl SseStream {
    fn new(byte_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(byte_stream),
            buffer: String::new(),
        }
    }
}

impl Stream for SseStream {
    type Item = Result<ApiStreamEvent, ApiError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            // Try to extract a complete SSE event from the buffer
            if let Some(event) = parse_next_sse_event(&mut this.buffer) {
                return Poll::Ready(Some(event));
            }

            // Need more data from the byte stream
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    match std::str::from_utf8(&chunk) {
                        Ok(text) => this.buffer.push_str(text),
                        Err(e) => {
                            return Poll::Ready(Some(Err(ApiError::ParseError(
                                format!("Invalid UTF-8 in SSE stream: {}", e),
                            ))));
                        }
                    }
                    // Loop back to try parsing again with new data
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(ApiError::NetworkError(e.to_string()))));
                }
                Poll::Ready(None) => {
                    // Stream ended
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

/// Parse the next complete SSE event from the buffer.
/// Returns None if no complete event is available yet.
fn parse_next_sse_event(buffer: &mut String) -> Option<Result<ApiStreamEvent, ApiError>> {
    // SSE events are separated by double newlines
    let event_end = buffer.find("\n\n")?;
    let event_block = buffer[..event_end].to_string();
    // Remove the consumed event + the double newline separator
    *buffer = buffer[event_end + 2..].to_string();

    let mut data_line: Option<&str> = None;

    for line in event_block.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("event:") {
            // We skip event type lines; we rely on the "type" field in the JSON data
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            let rest = rest.trim();
            data_line = Some(rest);
        }
    }

    let data = data_line?;

    // Handle the [DONE] sentinel
    if data == "[DONE]" {
        return None;
    }

    // Parse the JSON data into an ApiStreamEvent
    match serde_json::from_str::<ApiStreamEvent>(data) {
        Ok(event) => Some(Ok(event)),
        Err(e) => Some(Err(ApiError::ParseError(format!(
            "Failed to parse SSE event JSON: {} (data: {})",
            e, data
        )))),
    }
}

// --- Client implementation ---

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_MAX_RETRIES: u32 = 3;
const API_VERSION: &str = "2023-06-01";

impl AnthropicClient {
    /// Creates a new AnthropicClient with the given configuration.
    /// The HTTP client is configured for HTTP/2.
    pub fn new(config: ApiClientConfig) -> Self {
        let http_client = reqwest::Client::builder()
            .http2_prior_knowledge()
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http_client,
            api_key: config.api_key,
            base_url: config
                .base_url
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            max_retries: config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
        }
    }

    /// Returns the configured max retries.
    pub fn max_retries(&self) -> u32 {
        self.max_retries
    }

    /// Sends a streaming message creation request and returns an async stream of SSE events.
    pub async fn create_message_stream(
        &self,
        request: CreateMessageRequest,
    ) -> Result<SseStream, ApiError> {
        let url = format!("{}/v1/messages", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&request)
            .send()
            .await
            .map_err(|e| ApiError::NetworkError(e.to_string()))?;

        let status = response.status().as_u16();

        if !response.status().is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());

            return Err(match status {
                401 => ApiError::AuthError,
                429 => ApiError::RateLimited,
                400 => ApiError::BadRequest { message },
                500..=599 => ApiError::ServerError { status },
                _ => ApiError::HttpError { status, message },
            });
        }

        let byte_stream = response.bytes_stream();
        Ok(SseStream::new(byte_stream))
    }
}

// --- Convenience functions ---

/// Returns true if the given error is retryable.
pub fn is_retryable(error: &ApiError) -> bool {
    error.is_retryable()
}

/// Returns true if the given error is an auth error.
pub fn is_auth_error(error: &ApiError) -> bool {
    error.is_auth_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ApiStreamEvent deserialization tests ---

    #[test]
    fn test_deserialize_message_start() {
        let json = r#"{"type":"message_start","message":{"id":"msg_123","role":"assistant"}}"#;
        let event: ApiStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ApiStreamEvent::MessageStart { message } => {
                assert_eq!(message["id"], "msg_123");
                assert_eq!(message["role"], "assistant");
            }
            _ => panic!("Expected MessageStart"),
        }
    }

    #[test]
    fn test_deserialize_content_block_start() {
        let json = r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
        let event: ApiStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ApiStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                assert_eq!(index, 0);
                assert_eq!(content_block["type"], "text");
            }
            _ => panic!("Expected ContentBlockStart"),
        }
    }

    #[test]
    fn test_deserialize_content_block_delta() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let event: ApiStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ApiStreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                assert_eq!(delta["text"], "Hello");
            }
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    #[test]
    fn test_deserialize_content_block_stop() {
        let json = r#"{"type":"content_block_stop","index":0}"#;
        let event: ApiStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ApiStreamEvent::ContentBlockStop { index } => {
                assert_eq!(index, 0);
            }
            _ => panic!("Expected ContentBlockStop"),
        }
    }

    #[test]
    fn test_deserialize_message_delta() {
        let json = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}"#;
        let event: ApiStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ApiStreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta["stop_reason"], "end_turn");
                assert_eq!(usage["output_tokens"], 15);
            }
            _ => panic!("Expected MessageDelta"),
        }
    }

    #[test]
    fn test_deserialize_message_stop() {
        let json = r#"{"type":"message_stop"}"#;
        let event: ApiStreamEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, ApiStreamEvent::MessageStop));
    }

    #[test]
    fn test_deserialize_ping() {
        let json = r#"{"type":"ping"}"#;
        let event: ApiStreamEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, ApiStreamEvent::Ping));
    }

    #[test]
    fn test_deserialize_error_event() {
        let json = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let event: ApiStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            ApiStreamEvent::Error { error } => {
                assert_eq!(error.error_type, "overloaded_error");
                assert_eq!(error.message, "Overloaded");
            }
            _ => panic!("Expected Error"),
        }
    }

    // --- SSE parsing tests ---

    #[test]
    fn test_parse_sse_event_basic() {
        let mut buffer = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}\n\n".to_string();
        let result = parse_next_sse_event(&mut buffer);
        assert!(result.is_some());
        let event = result.unwrap().unwrap();
        match event {
            ApiStreamEvent::MessageStart { message } => {
                assert_eq!(message["id"], "msg_1");
            }
            _ => panic!("Expected MessageStart"),
        }
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_parse_sse_event_data_only() {
        let mut buffer =
            "data: {\"type\":\"ping\"}\n\n".to_string();
        let result = parse_next_sse_event(&mut buffer);
        assert!(result.is_some());
        let event = result.unwrap().unwrap();
        assert!(matches!(event, ApiStreamEvent::Ping));
    }

    #[test]
    fn test_parse_sse_event_done_sentinel() {
        let mut buffer = "data: [DONE]\n\n".to_string();
        let result = parse_next_sse_event(&mut buffer);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_sse_event_incomplete_buffer() {
        let mut buffer = "event: ping\ndata: {\"type\":\"pi".to_string();
        let result = parse_next_sse_event(&mut buffer);
        assert!(result.is_none());
        // Buffer should be unchanged
        assert_eq!(buffer, "event: ping\ndata: {\"type\":\"pi");
    }

    #[test]
    fn test_parse_sse_event_multiple_events() {
        let mut buffer = "data: {\"type\":\"ping\"}\n\ndata: {\"type\":\"message_stop\"}\n\n".to_string();

        let first = parse_next_sse_event(&mut buffer);
        assert!(first.is_some());
        assert!(matches!(first.unwrap().unwrap(), ApiStreamEvent::Ping));

        let second = parse_next_sse_event(&mut buffer);
        assert!(second.is_some());
        assert!(matches!(
            second.unwrap().unwrap(),
            ApiStreamEvent::MessageStop
        ));

        assert!(buffer.is_empty());
    }

    #[test]
    fn test_parse_sse_event_invalid_json() {
        let mut buffer = "data: {not valid json}\n\n".to_string();
        let result = parse_next_sse_event(&mut buffer);
        assert!(result.is_some());
        let err = result.unwrap().unwrap_err();
        assert!(matches!(err, ApiError::ParseError(_)));
    }

    #[test]
    fn test_parse_sse_event_empty_lines_skipped() {
        let mut buffer = "\n\nevent: ping\n\ndata: {\"type\":\"ping\"}\n\n".to_string();
        // First double-newline produces an event block with no data line -> None
        let first = parse_next_sse_event(&mut buffer);
        assert!(first.is_none());
        // The "event: ping" block also has no data line
        let second = parse_next_sse_event(&mut buffer);
        assert!(second.is_none());
        // Now the actual data event
        let third = parse_next_sse_event(&mut buffer);
        assert!(third.is_some());
        assert!(matches!(third.unwrap().unwrap(), ApiStreamEvent::Ping));
    }

    // --- Retry classification tests ---

    #[test]
    fn test_is_retryable_rate_limited() {
        let err = ApiError::RateLimited;
        assert!(err.is_retryable());
        assert!(is_retryable(&err));
    }

    #[test]
    fn test_is_retryable_server_error() {
        let err = ApiError::ServerError { status: 500 };
        assert!(err.is_retryable());
        assert!(is_retryable(&err));

        let err = ApiError::ServerError { status: 503 };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_not_retryable_auth_error() {
        let err = ApiError::AuthError;
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_not_retryable_bad_request() {
        let err = ApiError::BadRequest {
            message: "invalid model".to_string(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_not_retryable_parse_error() {
        let err = ApiError::ParseError("bad json".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_not_retryable_network_error() {
        let err = ApiError::NetworkError("connection refused".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_auth_error() {
        assert!(ApiError::AuthError.is_auth_error());
        assert!(is_auth_error(&ApiError::AuthError));
    }

    #[test]
    fn test_is_not_auth_error() {
        assert!(!ApiError::RateLimited.is_auth_error());
        assert!(!ApiError::ServerError { status: 500 }.is_auth_error());
        assert!(!ApiError::BadRequest {
            message: "bad".to_string()
        }
        .is_auth_error());
        assert!(!ApiError::NetworkError("err".to_string()).is_auth_error());
        assert!(!ApiError::ParseError("err".to_string()).is_auth_error());
    }

    // --- Client construction tests ---

    #[test]
    fn test_client_default_config() {
        let client = AnthropicClient::new(ApiClientConfig {
            api_key: "test-key".to_string(),
            base_url: None,
            max_retries: None,
        });
        assert_eq!(client.base_url, DEFAULT_BASE_URL);
        assert_eq!(client.max_retries, DEFAULT_MAX_RETRIES);
    }

    #[test]
    fn test_client_custom_config() {
        let client = AnthropicClient::new(ApiClientConfig {
            api_key: "sk-ant-test".to_string(),
            base_url: Some("https://custom.api.com".to_string()),
            max_retries: Some(5),
        });
        assert_eq!(client.base_url, "https://custom.api.com");
        assert_eq!(client.max_retries, 5);
    }

    // --- CreateMessageRequest serialization tests ---

    #[test]
    fn test_create_message_request_serialization() {
        let request = CreateMessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
            system: None,
            tools: None,
            max_tokens: 4096,
            stream: true,
            thinking: None,
            metadata: None,
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "claude-sonnet-4-20250514");
        assert_eq!(json["max_tokens"], 4096);
        assert_eq!(json["stream"], true);
        // Optional None fields should be absent
        assert!(json.get("system").is_none());
        assert!(json.get("tools").is_none());
        assert!(json.get("thinking").is_none());
        assert!(json.get("metadata").is_none());
    }

    #[test]
    fn test_create_message_request_with_all_fields() {
        let request = CreateMessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
            system: Some(vec![serde_json::json!({"type": "text", "text": "You are helpful."})]),
            tools: Some(vec![serde_json::json!({"name": "bash", "description": "Run bash"})]),
            max_tokens: 8192,
            stream: true,
            thinking: Some(serde_json::json!({"type": "enabled", "budget_tokens": 1024})),
            metadata: Some(serde_json::json!({"user_id": "test"})),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert!(json.get("system").is_some());
        assert!(json.get("tools").is_some());
        assert!(json.get("thinking").is_some());
        assert!(json.get("metadata").is_some());
    }

    // --- ApiErrorDetail tests ---

    #[test]
    fn test_api_error_detail_deserialization() {
        let json = r#"{"type":"overloaded_error","message":"The API is overloaded"}"#;
        let detail: ApiErrorDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.error_type, "overloaded_error");
        assert_eq!(detail.message, "The API is overloaded");
    }

    // --- ApiError Display tests ---

    #[test]
    fn test_api_error_display() {
        assert_eq!(
            format!("{}", ApiError::AuthError),
            "Auth error (401)"
        );
        assert_eq!(
            format!("{}", ApiError::RateLimited),
            "Rate limited (429)"
        );
        assert_eq!(
            format!("{}", ApiError::ServerError { status: 503 }),
            "Server error (503)"
        );
        assert_eq!(
            format!("{}", ApiError::BadRequest { message: "invalid".to_string() }),
            "Bad request (400): invalid"
        );
    }
}
