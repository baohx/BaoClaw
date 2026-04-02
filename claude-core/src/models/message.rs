use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

// --- Error types ---

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "validation error on '{}': {}", self.field, self.message)
    }
}

impl std::error::Error for ValidationError {}

// --- Core message types ---

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub uuid: String,
    pub timestamp: String,
    #[serde(flatten)]
    pub content: MessageContent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageContent {
    #[serde(rename = "user")]
    User {
        message: ApiUserMessage,
        #[serde(default)]
        is_meta: bool,
        tool_use_result: Option<ToolUseResult>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        message: ApiAssistantMessage,
        #[serde(default)]
        cost_usd: f64,
        #[serde(default)]
        duration_ms: u64,
    },
    #[serde(rename = "system")]
    System {
        subtype: SystemSubtype,
        content: String,
    },
    #[serde(rename = "progress")]
    Progress {
        tool_use_id: String,
        data: Value,
    },
}

// --- API message types ---

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiUserMessage {
    pub role: String,
    pub content: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiAssistantMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

// --- Supporting types ---

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolUseResult {
    pub tool_use_id: String,
    pub output: Value,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SystemSubtype {
    CompactBoundary,
    LocalCommand,
    Attachment,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

// --- Validation functions ---

/// Validates that a string is a valid UUID v4 format.
pub fn is_valid_uuid_v4(s: &str) -> bool {
    // UUID v4 format: 8-4-4-4-12 hex digits, version nibble = 4, variant bits = 8/9/a/b
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    for (part, &expected_len) in parts.iter().zip(&expected_lens) {
        if part.len() != expected_len {
            return false;
        }
        if !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }
    // Version nibble (first char of 3rd group) must be '4'
    if !parts[2].starts_with('4') {
        return false;
    }
    // Variant bits (first char of 4th group) must be 8, 9, a, or b
    let variant_char = parts[3].chars().next().unwrap();
    matches!(variant_char, '8' | '9' | 'a' | 'b' | 'A' | 'B')
}

/// Validates that a string is a valid ISO 8601 timestamp.
pub fn is_valid_iso8601(s: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(s).is_ok()
}

impl Message {
    /// Validates the message fields.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !is_valid_uuid_v4(&self.uuid) {
            return Err(ValidationError {
                field: "uuid".to_string(),
                message: "invalid UUID v4 format".to_string(),
            });
        }
        if !is_valid_iso8601(&self.timestamp) {
            return Err(ValidationError {
                field: "timestamp".to_string(),
                message: "invalid ISO 8601 timestamp".to_string(),
            });
        }
        if let MessageContent::Assistant { cost_usd, .. } = &self.content {
            if *cost_usd < 0.0 {
                return Err(ValidationError {
                    field: "cost_usd".to_string(),
                    message: "cost_usd must be >= 0".to_string(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_uuid_v4() {
        assert!(is_valid_uuid_v4("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_uuid_v4("6ba7b810-9dad-41d4-80b4-00c04fd430c8"));
        // Invalid: wrong version
        assert!(!is_valid_uuid_v4("550e8400-e29b-31d4-a716-446655440000"));
        // Invalid: wrong variant
        assert!(!is_valid_uuid_v4("550e8400-e29b-41d4-c716-446655440000"));
        // Invalid: wrong length
        assert!(!is_valid_uuid_v4("550e8400-e29b-41d4-a716-44665544000"));
        // Invalid: not hex
        assert!(!is_valid_uuid_v4("550e8400-e29b-41d4-a716-44665544000g"));
        // Empty
        assert!(!is_valid_uuid_v4(""));
    }

    #[test]
    fn test_is_valid_iso8601() {
        assert!(is_valid_iso8601("2024-01-15T10:30:00Z"));
        assert!(is_valid_iso8601("2024-01-15T10:30:00+08:00"));
        assert!(is_valid_iso8601("2024-01-15T10:30:00.123Z"));
        assert!(!is_valid_iso8601("2024-01-15"));
        assert!(!is_valid_iso8601("not a date"));
        assert!(!is_valid_iso8601(""));
    }

    #[test]
    fn test_message_validate_valid_user() {
        let msg = Message {
            uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            content: MessageContent::User {
                message: ApiUserMessage {
                    role: "user".to_string(),
                    content: Value::String("hello".to_string()),
                },
                is_meta: false,
                tool_use_result: None,
            },
        };
        assert!(msg.validate().is_ok());
    }

    #[test]
    fn test_message_validate_valid_assistant() {
        let msg = Message {
            uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "hi".to_string(),
                    }],
                    stop_reason: Some("end_turn".to_string()),
                    usage: Some(Usage {
                        input_tokens: 10,
                        output_tokens: 5,
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    }),
                },
                cost_usd: 0.001,
                duration_ms: 500,
            },
        };
        assert!(msg.validate().is_ok());
    }

    #[test]
    fn test_message_validate_invalid_uuid() {
        let msg = Message {
            uuid: "not-a-uuid".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            content: MessageContent::System {
                subtype: SystemSubtype::LocalCommand,
                content: "test".to_string(),
            },
        };
        let err = msg.validate().unwrap_err();
        assert_eq!(err.field, "uuid");
    }

    #[test]
    fn test_message_validate_invalid_timestamp() {
        let msg = Message {
            uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: "bad-timestamp".to_string(),
            content: MessageContent::System {
                subtype: SystemSubtype::Attachment,
                content: "test".to_string(),
            },
        };
        let err = msg.validate().unwrap_err();
        assert_eq!(err.field, "timestamp");
    }

    #[test]
    fn test_message_validate_negative_cost() {
        let msg = Message {
            uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".to_string(),
                    content: vec![],
                    stop_reason: None,
                    usage: None,
                },
                cost_usd: -1.0,
                duration_ms: 0,
            },
        };
        let err = msg.validate().unwrap_err();
        assert_eq!(err.field, "cost_usd");
    }

    #[test]
    fn test_message_serialization_roundtrip() {
        let msg = Message {
            uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            content: MessageContent::User {
                message: ApiUserMessage {
                    role: "user".to_string(),
                    content: Value::String("hello".to_string()),
                },
                is_meta: false,
                tool_use_result: None,
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg.uuid, deserialized.uuid);
        assert_eq!(msg.timestamp, deserialized.timestamp);
    }

    #[test]
    fn test_content_block_serialization() {
        let text = ContentBlock::Text {
            text: "hello".to_string(),
        };
        let json = serde_json::to_string(&text).unwrap();
        assert!(json.contains("\"type\":\"text\""));

        let tool_use = ContentBlock::ToolUse {
            id: "tu_1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&tool_use).unwrap();
        assert!(json.contains("\"type\":\"tool_use\""));

        let thinking = ContentBlock::Thinking {
            thinking: "let me think".to_string(),
        };
        let json = serde_json::to_string(&thinking).unwrap();
        assert!(json.contains("\"type\":\"thinking\""));
    }
}
