use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Request ID - can be number or string
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

/// JSON-RPC 2.0 Request
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    pub id: RequestId,
}

/// JSON-RPC 2.0 Response (success)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Value,
    pub id: RequestId,
}

impl JsonRpcResponse {
    /// Create a success response
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result,
            id,
        }
    }
}

/// JSON-RPC 2.0 Error object
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 Error Response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcErrorResponse {
    pub jsonrpc: String,
    pub error: JsonRpcError,
    pub id: Option<RequestId>,
}

impl JsonRpcErrorResponse {
    /// Create an error response
    pub fn new(id: Option<RequestId>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            error: JsonRpcError {
                code,
                message,
                data: None,
            },
            id,
        }
    }
}

/// JSON-RPC 2.0 Notification (no id)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcNotification {
    /// Create a new notification
    pub fn new(method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        }
    }
}

/// Unified message type for parsing incoming messages
#[derive(Clone, Debug)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    ErrorResponse(JsonRpcErrorResponse),
    Notification(JsonRpcNotification),
}

impl<'de> Deserialize<'de> for JsonRpcMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        let obj = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("JSON-RPC message must be an object"))?;

        let has_method = obj.contains_key("method");
        let has_id = obj.contains_key("id") && !obj["id"].is_null();
        let has_result = obj.contains_key("result");
        let has_error = obj.contains_key("error");

        if has_error {
            // Error response
            let msg: JsonRpcErrorResponse =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            Ok(JsonRpcMessage::ErrorResponse(msg))
        } else if has_result {
            // Success response
            let msg: JsonRpcResponse =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            Ok(JsonRpcMessage::Response(msg))
        } else if has_method && has_id {
            // Request (has method + id)
            let msg: JsonRpcRequest =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            Ok(JsonRpcMessage::Request(msg))
        } else if has_method {
            // Notification (has method, no id)
            let msg: JsonRpcNotification =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            Ok(JsonRpcMessage::Notification(msg))
        } else {
            Err(serde::de::Error::custom(
                "Cannot determine JSON-RPC message type: missing 'method', 'result', or 'error' field",
            ))
        }
    }
}

impl Serialize for JsonRpcMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            JsonRpcMessage::Request(req) => req.serialize(serializer),
            JsonRpcMessage::Response(resp) => resp.serialize(serializer),
            JsonRpcMessage::ErrorResponse(err) => err.serialize(serializer),
            JsonRpcMessage::Notification(notif) => notif.serialize(serializer),
        }
    }
}

/// Encode a message as NDJSON (JSON + newline)
pub fn encode_ndjson(message: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Decode a single NDJSON line into a JsonRpcMessage
pub fn decode_ndjson_line(line: &str) -> Result<JsonRpcMessage, serde_json::Error> {
    let trimmed = line.trim();
    serde_json::from_str(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- RequestId tests ---

    #[test]
    fn test_request_id_number_serialization() {
        let id = RequestId::Number(42);
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, json!(42));
    }

    #[test]
    fn test_request_id_string_serialization() {
        let id = RequestId::String("abc-123".to_string());
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, json!("abc-123"));
    }

    #[test]
    fn test_request_id_number_deserialization() {
        let id: RequestId = serde_json::from_value(json!(99)).unwrap();
        assert_eq!(id, RequestId::Number(99));
    }

    #[test]
    fn test_request_id_string_deserialization() {
        let id: RequestId = serde_json::from_value(json!("req-1")).unwrap();
        assert_eq!(id, RequestId::String("req-1".to_string()));
    }

    // --- JsonRpcRequest tests ---

    #[test]
    fn test_request_serialization_roundtrip() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "submitMessage".to_string(),
            params: json!({"prompt": "hello"}),
            id: RequestId::Number(1),
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.jsonrpc, "2.0");
        assert_eq!(parsed.method, "submitMessage");
        assert_eq!(parsed.params, json!({"prompt": "hello"}));
        assert_eq!(parsed.id, RequestId::Number(1));
    }

    #[test]
    fn test_request_default_params() {
        let json_str = r#"{"jsonrpc":"2.0","method":"abort","id":5}"#;
        let req: JsonRpcRequest = serde_json::from_str(json_str).unwrap();
        assert_eq!(req.params, Value::Null);
    }

    // --- JsonRpcResponse tests ---

    #[test]
    fn test_response_success_constructor() {
        let resp = JsonRpcResponse::success(RequestId::Number(1), json!({"status": "ok"}));
        assert_eq!(resp.jsonrpc, "2.0");
        assert_eq!(resp.result, json!({"status": "ok"}));
        assert_eq!(resp.id, RequestId::Number(1));
    }

    #[test]
    fn test_response_serialization_roundtrip() {
        let resp = JsonRpcResponse::success(RequestId::String("r1".into()), json!("done"));
        let json_str = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.id, RequestId::String("r1".into()));
        assert_eq!(parsed.result, json!("done"));
    }

    // --- JsonRpcErrorResponse tests ---

    #[test]
    fn test_error_response_constructor() {
        let err = JsonRpcErrorResponse::new(Some(RequestId::Number(2)), -32600, "Invalid Request".into());
        assert_eq!(err.jsonrpc, "2.0");
        assert_eq!(err.error.code, -32600);
        assert_eq!(err.error.message, "Invalid Request");
        assert!(err.error.data.is_none());
        assert_eq!(err.id, Some(RequestId::Number(2)));
    }

    #[test]
    fn test_error_response_null_id() {
        let err = JsonRpcErrorResponse::new(None, -32700, "Parse error".into());
        assert!(err.id.is_none());
    }

    #[test]
    fn test_error_response_serialization_roundtrip() {
        let err = JsonRpcErrorResponse {
            jsonrpc: "2.0".to_string(),
            error: JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
                data: Some(json!({"detail": "unknown method"})),
            },
            id: Some(RequestId::Number(3)),
        };
        let json_str = serde_json::to_string(&err).unwrap();
        let parsed: JsonRpcErrorResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.error.code, -32601);
        assert_eq!(parsed.error.data, Some(json!({"detail": "unknown method"})));
    }

    #[test]
    fn test_error_data_skipped_when_none() {
        let err = JsonRpcErrorResponse::new(None, -32700, "Parse error".into());
        let json_str = serde_json::to_string(&err).unwrap();
        assert!(!json_str.contains("\"data\""));
    }

    // --- JsonRpcNotification tests ---

    #[test]
    fn test_notification_constructor() {
        let notif = JsonRpcNotification::new("stream/event", json!({"type": "assistant_chunk"}));
        assert_eq!(notif.jsonrpc, "2.0");
        assert_eq!(notif.method, "stream/event");
        assert_eq!(notif.params, json!({"type": "assistant_chunk"}));
    }

    #[test]
    fn test_notification_serialization_roundtrip() {
        let notif = JsonRpcNotification::new("state/patch", json!({"patches": []}));
        let json_str = serde_json::to_string(&notif).unwrap();
        let parsed: JsonRpcNotification = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.method, "state/patch");
    }

    #[test]
    fn test_notification_default_params() {
        let json_str = r#"{"jsonrpc":"2.0","method":"ping"}"#;
        let notif: JsonRpcNotification = serde_json::from_str(json_str).unwrap();
        assert_eq!(notif.params, Value::Null);
    }

    // --- JsonRpcMessage deserialization tests ---

    #[test]
    fn test_message_deserialize_request() {
        let json_str = r#"{"jsonrpc":"2.0","method":"initialize","params":{"cwd":"/tmp"},"id":1}"#;
        let msg: JsonRpcMessage = serde_json::from_str(json_str).unwrap();
        match msg {
            JsonRpcMessage::Request(req) => {
                assert_eq!(req.method, "initialize");
                assert_eq!(req.id, RequestId::Number(1));
            }
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_message_deserialize_response() {
        let json_str = r#"{"jsonrpc":"2.0","result":{"capabilities":{}},"id":1}"#;
        let msg: JsonRpcMessage = serde_json::from_str(json_str).unwrap();
        match msg {
            JsonRpcMessage::Response(resp) => {
                assert_eq!(resp.id, RequestId::Number(1));
                assert_eq!(resp.result, json!({"capabilities": {}}));
            }
            _ => panic!("Expected Response"),
        }
    }

    #[test]
    fn test_message_deserialize_error_response() {
        let json_str = r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#;
        let msg: JsonRpcMessage = serde_json::from_str(json_str).unwrap();
        match msg {
            JsonRpcMessage::ErrorResponse(err) => {
                assert_eq!(err.error.code, -32601);
                assert_eq!(err.id, Some(RequestId::Number(1)));
            }
            _ => panic!("Expected ErrorResponse"),
        }
    }

    #[test]
    fn test_message_deserialize_error_response_null_id() {
        let json_str = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":null}"#;
        let msg: JsonRpcMessage = serde_json::from_str(json_str).unwrap();
        match msg {
            JsonRpcMessage::ErrorResponse(err) => {
                assert_eq!(err.error.code, -32700);
                assert!(err.id.is_none());
            }
            _ => panic!("Expected ErrorResponse"),
        }
    }

    #[test]
    fn test_message_deserialize_notification() {
        let json_str = r#"{"jsonrpc":"2.0","method":"stream/event","params":{"type":"result"}}"#;
        let msg: JsonRpcMessage = serde_json::from_str(json_str).unwrap();
        match msg {
            JsonRpcMessage::Notification(notif) => {
                assert_eq!(notif.method, "stream/event");
            }
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn test_message_deserialize_request_with_string_id() {
        let json_str = r#"{"jsonrpc":"2.0","method":"test","params":{},"id":"uuid-abc"}"#;
        let msg: JsonRpcMessage = serde_json::from_str(json_str).unwrap();
        match msg {
            JsonRpcMessage::Request(req) => {
                assert_eq!(req.id, RequestId::String("uuid-abc".into()));
            }
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_message_deserialize_invalid_no_fields() {
        let json_str = r#"{"jsonrpc":"2.0"}"#;
        let result: Result<JsonRpcMessage, _> = serde_json::from_str(json_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_message_deserialize_not_object() {
        let result: Result<JsonRpcMessage, _> = serde_json::from_str("42");
        assert!(result.is_err());
    }

    // --- JsonRpcMessage serialization tests ---

    #[test]
    fn test_message_serialize_request() {
        let msg = JsonRpcMessage::Request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "test".to_string(),
            params: json!({}),
            id: RequestId::Number(1),
        });
        let json_str = serde_json::to_string(&msg).unwrap();
        assert!(json_str.contains("\"method\":\"test\""));
        assert!(json_str.contains("\"id\":1"));
    }

    #[test]
    fn test_message_serialize_notification() {
        let msg = JsonRpcMessage::Notification(JsonRpcNotification::new("ping", json!(null)));
        let json_str = serde_json::to_string(&msg).unwrap();
        assert!(json_str.contains("\"method\":\"ping\""));
        assert!(!json_str.contains("\"id\""));
    }

    // --- NDJSON tests ---

    #[test]
    fn test_encode_ndjson_ends_with_newline() {
        let notif = JsonRpcNotification::new("test", json!({}));
        let bytes = encode_ndjson(&notif).unwrap();
        assert_eq!(*bytes.last().unwrap(), b'\n');
    }

    #[test]
    fn test_encode_ndjson_valid_json_before_newline() {
        let resp = JsonRpcResponse::success(RequestId::Number(1), json!("ok"));
        let bytes = encode_ndjson(&resp).unwrap();
        let line = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(line).unwrap();
        assert_eq!(parsed.result, json!("ok"));
    }

    #[test]
    fn test_decode_ndjson_line_request() {
        let line = r#"{"jsonrpc":"2.0","method":"submitMessage","params":{"prompt":"hi"},"id":1}"#;
        let msg = decode_ndjson_line(line).unwrap();
        match msg {
            JsonRpcMessage::Request(req) => assert_eq!(req.method, "submitMessage"),
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_decode_ndjson_line_with_trailing_whitespace() {
        let line = r#"{"jsonrpc":"2.0","method":"ping"}  "#;
        let msg = decode_ndjson_line(line).unwrap();
        match msg {
            JsonRpcMessage::Notification(notif) => assert_eq!(notif.method, "ping"),
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn test_decode_ndjson_line_with_newline() {
        let line = "{\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n";
        let msg = decode_ndjson_line(line).unwrap();
        match msg {
            JsonRpcMessage::Notification(notif) => assert_eq!(notif.method, "ping"),
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn test_ndjson_roundtrip_request() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: json!({"cwd": "/home/user"}),
            id: RequestId::Number(1),
        };
        let bytes = encode_ndjson(&req).unwrap();
        let line = std::str::from_utf8(&bytes).unwrap();
        let msg = decode_ndjson_line(line).unwrap();
        match msg {
            JsonRpcMessage::Request(parsed) => {
                assert_eq!(parsed.method, "initialize");
                assert_eq!(parsed.params, json!({"cwd": "/home/user"}));
                assert_eq!(parsed.id, RequestId::Number(1));
            }
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_ndjson_roundtrip_notification() {
        let notif = JsonRpcNotification::new("stream/event", json!({"type": "assistant_chunk", "content": "Hello"}));
        let bytes = encode_ndjson(&notif).unwrap();
        let line = std::str::from_utf8(&bytes).unwrap();
        let msg = decode_ndjson_line(line).unwrap();
        match msg {
            JsonRpcMessage::Notification(parsed) => {
                assert_eq!(parsed.method, "stream/event");
                assert_eq!(parsed.params["type"], "assistant_chunk");
            }
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn test_ndjson_roundtrip_response() {
        let resp = JsonRpcResponse::success(RequestId::String("r-42".into()), json!({"status": "complete"}));
        let bytes = encode_ndjson(&resp).unwrap();
        let line = std::str::from_utf8(&bytes).unwrap();
        let msg = decode_ndjson_line(line).unwrap();
        match msg {
            JsonRpcMessage::Response(parsed) => {
                assert_eq!(parsed.id, RequestId::String("r-42".into()));
                assert_eq!(parsed.result["status"], "complete");
            }
            _ => panic!("Expected Response"),
        }
    }

    #[test]
    fn test_ndjson_roundtrip_error_response() {
        let err = JsonRpcErrorResponse::new(Some(RequestId::Number(5)), -32600, "Invalid Request".into());
        let bytes = encode_ndjson(&err).unwrap();
        let line = std::str::from_utf8(&bytes).unwrap();
        let msg = decode_ndjson_line(line).unwrap();
        match msg {
            JsonRpcMessage::ErrorResponse(parsed) => {
                assert_eq!(parsed.error.code, -32600);
                assert_eq!(parsed.error.message, "Invalid Request");
            }
            _ => panic!("Expected ErrorResponse"),
        }
    }

    #[test]
    fn test_decode_ndjson_invalid_json() {
        let result = decode_ndjson_line("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_ndjson_empty_line() {
        let result = decode_ndjson_line("");
        assert!(result.is_err());
    }
}
