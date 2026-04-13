use serde_json::Value;

use crate::engine::query_engine::EngineEvent;
use crate::ipc::protocol::JsonRpcNotification;
use crate::state::manager::StatePatch;

/// Convert an EngineEvent to a JSON-RPC notification for the "stream/event" method.
pub fn engine_event_to_notification(event: &EngineEvent) -> JsonRpcNotification {
    JsonRpcNotification::new(
        "stream/event",
        serde_json::to_value(event).unwrap_or(Value::Null),
    )
}

/// Convert a batch of StatePatches to a JSON-RPC notification for the "state/patch" method.
pub fn state_patches_to_notification(patches: &[StatePatch]) -> JsonRpcNotification {
    JsonRpcNotification::new(
        "state/patch",
        serde_json::json!({
            "patches": serde_json::to_value(patches).unwrap_or(Value::Array(vec![])),
        }),
    )
}

/// Helper to send an EngineEvent over an IpcConnection.
pub async fn send_engine_event(
    conn: &mut crate::ipc::server::IpcConnection,
    event: &EngineEvent,
) -> std::io::Result<()> {
    let notif = engine_event_to_notification(event);
    let params = serde_json::to_value(&notif.params).unwrap_or(Value::Null);
    conn.send_notification(&notif.method, params).await
}

/// Helper to send StatePatches over an IpcConnection.
pub async fn send_state_patches(
    conn: &mut crate::ipc::server::IpcConnection,
    patches: &[StatePatch],
) -> std::io::Result<()> {
    if patches.is_empty() {
        return Ok(());
    }
    let notif = state_patches_to_notification(patches);
    let params = serde_json::to_value(&notif.params).unwrap_or(Value::Null);
    conn.send_notification(&notif.method, params).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::query_engine::{EngineError, QueryResult, QueryStatus, EMPTY_USAGE};
    use crate::state::manager::PatchOp;
    use serde_json::json;

    // --- engine_event_to_notification tests ---

    #[test]
    fn test_assistant_chunk_notification_method() {
        let event = EngineEvent::AssistantChunk {
            content: "Hello".to_string(),
            tool_use_id: None,
        };
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.method, "stream/event");
        assert_eq!(notif.jsonrpc, "2.0");
    }

    #[test]
    fn test_assistant_chunk_notification_params() {
        let event = EngineEvent::AssistantChunk {
            content: "Hello world".to_string(),
            tool_use_id: None,
        };
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.params["type"], "assistant_chunk");
        assert_eq!(notif.params["content"], "Hello world");
    }

    #[test]
    fn test_tool_use_notification() {
        let event = EngineEvent::ToolUse {
            tool_name: "Bash".to_string(),
            input: json!({"command": "ls -la"}),
            tool_use_id: "tu_001".to_string(),
        };
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.method, "stream/event");
        assert_eq!(notif.params["type"], "tool_use");
        assert_eq!(notif.params["tool_name"], "Bash");
        assert_eq!(notif.params["input"]["command"], "ls -la");
        assert_eq!(notif.params["tool_use_id"], "tu_001");
    }

    #[test]
    fn test_tool_result_notification() {
        let event = EngineEvent::ToolResult {
            tool_use_id: "tu_002".to_string(),
            output: json!({"stdout": "file.txt\n"}),
            is_error: false,
        };
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.method, "stream/event");
        assert_eq!(notif.params["type"], "tool_result");
        assert_eq!(notif.params["tool_use_id"], "tu_002");
        assert!(!notif.params["is_error"].as_bool().unwrap());
    }

    #[test]
    fn test_tool_result_error_notification() {
        let event = EngineEvent::ToolResult {
            tool_use_id: "tu_003".to_string(),
            output: json!({"error": "command not found"}),
            is_error: true,
        };
        let notif = engine_event_to_notification(&event);
        assert!(notif.params["is_error"].as_bool().unwrap());
    }

    #[test]
    fn test_permission_request_notification() {
        let event = EngineEvent::PermissionRequest {
            tool_name: "FileWrite".to_string(),
            input: json!({"path": "/tmp/test.txt", "content": "hello"}),
            tool_use_id: "tu_004".to_string(),
        };
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.method, "stream/event");
        assert_eq!(notif.params["type"], "permission_request");
        assert_eq!(notif.params["tool_name"], "FileWrite");
    }

    #[test]
    fn test_progress_notification() {
        let event = EngineEvent::Progress {
            tool_use_id: "tu_005".to_string(),
            data: json!({"percent": 75}),
        };
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.params["type"], "progress");
        assert_eq!(notif.params["data"]["percent"], 75);
    }

    #[test]
    fn test_state_update_notification() {
        let event = EngineEvent::StateUpdate {
            patch: json!({"path": "/tasks/b12345678", "op": "replace"}),
        };
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.params["type"], "state_update");
    }

    #[test]
    fn test_result_notification() {
        let event = EngineEvent::Result(QueryResult {
            status: QueryStatus::Complete,
            text: Some("Done!".to_string()),
            stop_reason: Some("end_turn".to_string()),
            total_cost_usd: 0.01,
            usage: EMPTY_USAGE,
            num_turns: 2,
            duration_ms: 3000,
        });
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.method, "stream/event");
        assert_eq!(notif.params["type"], "result");
        assert_eq!(notif.params["status"], "complete");
        assert_eq!(notif.params["text"], "Done!");
        assert_eq!(notif.params["num_turns"], 2);
    }

    #[test]
    fn test_error_notification() {
        let event = EngineEvent::Error(EngineError {
            code: "rate_limit".to_string(),
            message: "Too many requests".to_string(),
            details: Some(json!({"retry_after": 30})),
        });
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.method, "stream/event");
        assert_eq!(notif.params["type"], "error");
        assert_eq!(notif.params["code"], "rate_limit");
        assert_eq!(notif.params["message"], "Too many requests");
        assert_eq!(notif.params["details"]["retry_after"], 30);
    }

    // --- state_patches_to_notification tests ---

    #[test]
    fn test_state_patches_notification_method() {
        let patches = vec![StatePatch {
            path: "/model".to_string(),
            op: PatchOp::Replace {
                value: Value::String("claude-4".to_string()),
            },
        }];
        let notif = state_patches_to_notification(&patches);
        assert_eq!(notif.method, "state/patch");
        assert_eq!(notif.jsonrpc, "2.0");
    }

    #[test]
    fn test_state_patches_notification_structure() {
        let patches = vec![
            StatePatch {
                path: "/model".to_string(),
                op: PatchOp::Replace {
                    value: Value::String("claude-4".to_string()),
                },
            },
            StatePatch {
                path: "/tasks/b12345678".to_string(),
                op: PatchOp::Add {
                    value: json!({"id": "b12345678", "status": "running"}),
                },
            },
        ];
        let notif = state_patches_to_notification(&patches);
        let patches_arr = notif.params["patches"].as_array().unwrap();
        assert_eq!(patches_arr.len(), 2);
        assert_eq!(patches_arr[0]["path"], "/model");
        assert_eq!(patches_arr[1]["path"], "/tasks/b12345678");
    }

    #[test]
    fn test_state_patches_empty_array() {
        let patches: Vec<StatePatch> = vec![];
        let notif = state_patches_to_notification(&patches);
        assert_eq!(notif.method, "state/patch");
        let patches_arr = notif.params["patches"].as_array().unwrap();
        assert!(patches_arr.is_empty());
    }

    #[test]
    fn test_state_patches_remove_op() {
        let patches = vec![StatePatch {
            path: "/tasks/b99999999".to_string(),
            op: PatchOp::Remove,
        }];
        let notif = state_patches_to_notification(&patches);
        let patches_arr = notif.params["patches"].as_array().unwrap();
        assert_eq!(patches_arr.len(), 1);
        assert_eq!(patches_arr[0]["path"], "/tasks/b99999999");
        assert_eq!(patches_arr[0]["op"], json!({"op": "remove"}));
    }

    // --- assistant_chunk with tool_use_id ---

    #[test]
    fn test_assistant_chunk_with_tool_use_id() {
        let event = EngineEvent::AssistantChunk {
            content: "partial".to_string(),
            tool_use_id: Some("tu_100".to_string()),
        };
        let notif = engine_event_to_notification(&event);
        assert_eq!(notif.params["tool_use_id"], "tu_100");
    }
}
