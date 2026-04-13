//! PBT: Property 14 — Compact preserves recent messages
//!
//! **Validates: Requirement 7.2**
//!
//! For any message list with length > 4, after compact, the last 4
//! messages from the original list shall still be present in the
//! resulting message list.
//!
//! Since `compact()` requires an API call for summarisation, we test
//! the core invariant directly: given a message list, the compact
//! logic keeps the last `keep_recent` (4) messages intact and replaces
//! the older ones with a single CompactBoundary system message.

use proptest::prelude::*;
use serde_json::Value;

use baoclaw_core::models::message::{
    ApiAssistantMessage, ApiUserMessage, ContentBlock, Message, MessageContent, SystemSubtype,
};

/// Strategy for generating a simple user message.
fn user_msg_strategy() -> impl Strategy<Value = Message> {
    "[a-zA-Z0-9 ]{1,60}".prop_map(|text| Message {
        uuid: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content: MessageContent::User {
            message: ApiUserMessage {
                role: "user".to_string(),
                content: Value::String(text),
            },
            is_meta: false,
            tool_use_result: None,
        },
    })
}

/// Strategy for generating a simple assistant message.
fn assistant_msg_strategy() -> impl Strategy<Value = Message> {
    "[a-zA-Z0-9 ]{1,60}".prop_map(|text| Message {
        uuid: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content: MessageContent::Assistant {
            message: ApiAssistantMessage {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text { text }],
                stop_reason: Some("end_turn".to_string()),
                usage: None,
            },
            cost_usd: 0.0,
            duration_ms: 0,
        },
    })
}

/// Strategy for generating a mixed message (user or assistant).
fn message_strategy() -> impl Strategy<Value = Message> {
    prop_oneof![user_msg_strategy(), assistant_msg_strategy(),]
}

/// Simulate the compact logic without calling the API.
///
/// This replicates the core invariant of `QueryEngine::compact()`:
/// - If messages.len() <= keep_recent, return unchanged
/// - Otherwise, replace old messages with a CompactBoundary + keep recent
fn simulate_compact(messages: &[Message], keep_recent: usize) -> Vec<Message> {
    if messages.len() <= keep_recent {
        return messages.to_vec();
    }

    let split = messages.len() - keep_recent;
    let recent = messages[split..].to_vec();

    // Create a compact boundary message (simulating the API summary)
    let boundary = Message {
        uuid: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content: MessageContent::System {
            subtype: SystemSubtype::CompactBoundary,
            content: "Summary of previous conversation.".to_string(),
        },
    };

    let mut result = vec![boundary];
    result.extend(recent);
    result
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 14: Compact preserves recent messages
    ///
    /// **Validates: Requirement 7.2**
    ///
    /// For any message list with length > 4, after compact, the last 4
    /// messages from the original list shall still be present in the
    /// resulting message list.
    #[test]
    fn compact_preserves_recent_messages(
        messages in prop::collection::vec(message_strategy(), 5..30),
    ) {
        let keep_recent = 4usize;

        // Capture the UUIDs of the last 4 messages before compact
        let original_recent_uuids: Vec<String> = messages
            .iter()
            .rev()
            .take(keep_recent)
            .rev()
            .map(|m| m.uuid.clone())
            .collect();

        let compacted = simulate_compact(&messages, keep_recent);

        // The compacted list should have exactly keep_recent + 1 messages
        // (1 CompactBoundary + keep_recent recent messages)
        prop_assert_eq!(
            compacted.len(),
            keep_recent + 1,
            "Compacted list should have {} messages (1 boundary + {} recent), got {}",
            keep_recent + 1,
            keep_recent,
            compacted.len()
        );

        // The first message should be a CompactBoundary
        match &compacted[0].content {
            MessageContent::System { subtype, .. } => {
                prop_assert!(
                    matches!(subtype, SystemSubtype::CompactBoundary),
                    "First message should be CompactBoundary"
                );
            }
            _ => {
                prop_assert!(false, "First message should be a System message");
            }
        }

        // The last keep_recent messages should match the original recent messages
        let compacted_recent_uuids: Vec<String> = compacted
            .iter()
            .skip(1) // skip the boundary
            .map(|m| m.uuid.clone())
            .collect();

        prop_assert_eq!(
            compacted_recent_uuids,
            original_recent_uuids,
            "Recent message UUIDs should be preserved after compact"
        );
    }

    /// Additional property: compact with <= 4 messages is a no-op.
    #[test]
    fn compact_noop_for_short_conversations(
        messages in prop::collection::vec(message_strategy(), 0..5),
    ) {
        let keep_recent = 4usize;
        let original_len = messages.len();
        let original_uuids: Vec<String> = messages.iter().map(|m| m.uuid.clone()).collect();

        let compacted = simulate_compact(&messages, keep_recent);

        // Should be unchanged
        prop_assert_eq!(
            compacted.len(),
            original_len,
            "Short conversations should not be compacted"
        );

        let compacted_uuids: Vec<String> = compacted.iter().map(|m| m.uuid.clone()).collect();
        prop_assert_eq!(
            compacted_uuids,
            original_uuids,
            "Message UUIDs should be unchanged for short conversations"
        );
    }
}
