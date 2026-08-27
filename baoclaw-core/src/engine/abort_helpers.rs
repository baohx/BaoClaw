//! Shared abort helpers for the query engine and tools.
//!
//! The abort signal is a `tokio::sync::watch::Sender<bool>` owned by `QueryEngine`.
//! When the user presses Ctrl+C, the CLI sends an IPC `abort` request, which
//! calls `QueryEngine::abort()` → sets the watch to `true`.
//!
//! All async loops that should be interruptible call `wait_for_abort(rx)` inside
//! a `tokio::select!` branch.

use crate::models::message::{ContentBlock, Message, MessageContent};
use tokio::sync::watch;

/// Await until the abort watch fires (value becomes `true`).
///
/// If the sender is dropped while the value is still `false`, this future
/// stays pending forever — that is intentional. A dropped sender without
/// setting `true` means the engine shut down cleanly, not that it aborted.
pub async fn wait_for_abort(mut rx: watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            // Sender dropped with value=false — not an abort, stay pending.
            std::future::pending::<()>().await;
        }
    }
}

/// Scan the tail of `messages` and fix any assistant message whose `tool_use`
/// blocks have no matching `tool_result`. Injects synthetic aborted
/// `tool_result` blocks to keep the history API-legal after a user abort.
///
/// Returns the number of orphan tool_use blocks that were patched.
pub fn cleanup_orphan_tool_uses(messages: &mut Vec<Message>) -> usize {
    let orphan_ids: Vec<String> = if let Some(last) = messages.last() {
        if let MessageContent::Assistant { message, .. } = &last.content {
            message
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        } else {
            return 0;
        }
    } else {
        return 0;
    };

    if orphan_ids.is_empty() {
        return 0;
    }

    // Build synthetic user message with aborted tool_results for every orphan
    let result_blocks: Vec<serde_json::Value> = orphan_ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": [{"type": "text", "text": "Tool execution aborted by user."}],
                "is_error": true,
            })
        })
        .collect();

    let synthetic_user = Message {
        uuid: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content: MessageContent::User {
            message: crate::models::message::ApiUserMessage {
                role: "user".to_string(),
                content: serde_json::Value::Array(result_blocks),
            },
            is_meta: true,
            tool_use_result: None,
        },
    };

    let count = orphan_ids.len();
    messages.push(synthetic_user);
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::{ApiAssistantMessage, ContentBlock, MessageContent};

    // ── wait_for_abort tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_wait_for_abort_fires_when_set() {
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(wait_for_abort(rx));
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        tx.send(true).unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_wait_for_abort_ignores_false_dropped_sender() {
        let (tx, rx) = watch::channel(false);
        drop(tx); // dropped with value=false
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(50), wait_for_abort(rx)).await;
        assert!(result.is_err(), "should have timed out (not aborted)");
    }

    // ── cleanup_orphan_tool_uses tests ────────────────────────────────────────

    #[test]
    fn test_cleanup_leaves_empty_history_alone() {
        let mut msgs: Vec<Message> = vec![];
        assert_eq!(cleanup_orphan_tool_uses(&mut msgs), 0);
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_cleanup_leaves_clean_assistant_alone() {
        // Assistant message with only text, no tool_use
        let asst = Message {
            uuid: "a1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "Hello".to_string(),
                    }],
                    stop_reason: None,
                    usage: None,
                },
                cost_usd: 0.0,
                duration_ms: 0,
            },
        };
        let mut msgs = vec![asst];
        assert_eq!(cleanup_orphan_tool_uses(&mut msgs), 0);
        assert_eq!(msgs.len(), 1); // no synthetic message added
    }

    #[test]
    fn test_cleanup_adds_synthetic_result_for_orphan_tool_use() {
        let asst = Message {
            uuid: "a1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "Bash".to_string(),
                        input: serde_json::json!({"command": "ls"}),
                    }],
                    stop_reason: None,
                    usage: None,
                },
                cost_usd: 0.0,
                duration_ms: 0,
            },
        };
        let mut msgs = vec![asst];
        let fixed = cleanup_orphan_tool_uses(&mut msgs);
        assert_eq!(fixed, 1);
        assert_eq!(msgs.len(), 2); // original + synthetic user
                                   // Verify the synthetic message is a user message
        assert!(matches!(msgs[1].content, MessageContent::User { .. }));
    }

    #[test]
    fn test_cleanup_handles_multiple_orphan_tool_uses() {
        let asst = Message {
            uuid: "a1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".to_string(),
                    content: vec![
                        ContentBlock::ToolUse {
                            id: "toolu_1".to_string(),
                            name: "Bash".to_string(),
                            input: serde_json::json!({}),
                        },
                        ContentBlock::ToolUse {
                            id: "toolu_2".to_string(),
                            name: "FileRead".to_string(),
                            input: serde_json::json!({}),
                        },
                    ],
                    stop_reason: None,
                    usage: None,
                },
                cost_usd: 0.0,
                duration_ms: 0,
            },
        };
        let mut msgs = vec![asst];
        let fixed = cleanup_orphan_tool_uses(&mut msgs);
        assert_eq!(fixed, 2);
        assert_eq!(msgs.len(), 2);
    }
}
