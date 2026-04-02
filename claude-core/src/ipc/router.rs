use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use super::protocol::JsonRpcRequest;

/// Errors that can occur during request routing
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("Unknown method: {0}")]
    UnknownMethod(String),
    #[error("Invalid params: {0}")]
    InvalidParams(String),
}

/// Client → Server RPC methods
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params")]
pub enum ClientMethod {
    #[serde(rename = "initialize")]
    Initialize {
        cwd: PathBuf,
        model: Option<String>,
        settings: Value,
        #[serde(default)]
        resume_session_id: Option<String>,
    },
    #[serde(rename = "submitMessage")]
    SubmitMessage {
        prompt: Value,
        uuid: Option<String>,
    },
    #[serde(rename = "permissionResponse")]
    PermissionResponse {
        tool_use_id: String,
        decision: String,
        rule: Option<String>,
    },
    #[serde(rename = "abort")]
    Abort,
    #[serde(rename = "updateSettings")]
    UpdateSettings { settings: Value },
    #[serde(rename = "shutdown")]
    Shutdown,
    #[serde(rename = "listTools")]
    ListTools,
    #[serde(rename = "listMcpServers")]
    ListMcpServers,
    #[serde(rename = "listSkills")]
    ListSkills,
    #[serde(rename = "listPlugins")]
    ListPlugins,
    #[serde(rename = "compact")]
    Compact,
    #[serde(rename = "switchModel")]
    SwitchModel { model: String },
    #[serde(rename = "gitDiff")]
    GitDiff,
    #[serde(rename = "gitCommit")]
    GitCommit { message: String },
    #[serde(rename = "gitStatus")]
    GitStatus,
    #[serde(rename = "listMcpResources")]
    ListMcpResources,
    #[serde(rename = "readMcpResource")]
    ReadMcpResource {
        server_name: String,
        uri: String,
    },
    #[serde(rename = "taskCreate")]
    TaskCreate {
        description: String,
        prompt: String,
    },
    #[serde(rename = "taskList")]
    TaskList,
    #[serde(rename = "taskStatus")]
    TaskStatus {
        task_id: String,
    },
    #[serde(rename = "taskStop")]
    TaskStop {
        task_id: String,
    },
}

/// Parse a JSON-RPC request into a ClientMethod
pub fn parse_client_method(request: &JsonRpcRequest) -> Result<ClientMethod, RouterError> {
    // Build a tagged representation that serde can deserialize via the
    // `#[serde(tag = "method", content = "params")]` attribute on ClientMethod.
    let tagged = serde_json::json!({
        "method": request.method,
        "params": request.params,
    });

    serde_json::from_value::<ClientMethod>(tagged).map_err(|e| {
        // Distinguish between unknown method and bad params.
        // If the method string doesn't match any variant, serde reports a
        // "no variant" / "unknown variant" error.
        let err_msg = e.to_string();
        if err_msg.contains("unknown variant") || err_msg.contains("no variant") {
            RouterError::UnknownMethod(request.method.clone())
        } else {
            RouterError::InvalidParams(err_msg)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::RequestId;
    use serde_json::json;

    /// Helper to build a JsonRpcRequest quickly
    fn make_request(method: &str, params: Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: RequestId::Number(1),
        }
    }

    // --- Successful parsing tests ---

    #[test]
    fn test_parse_initialize() {
        let req = make_request(
            "initialize",
            json!({
                "cwd": "/home/user/project",
                "model": "claude-sonnet-4-20250514",
                "settings": {"verbose": true}
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::Initialize {
                cwd,
                model,
                settings,
                ..
            } => {
                assert_eq!(cwd, PathBuf::from("/home/user/project"));
                assert_eq!(model, Some("claude-sonnet-4-20250514".to_string()));
                assert_eq!(settings, json!({"verbose": true}));
            }
            _ => panic!("Expected Initialize, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_initialize_without_optional_model() {
        let req = make_request(
            "initialize",
            json!({
                "cwd": "/tmp",
                "settings": {}
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::Initialize { model, .. } => {
                assert_eq!(model, None);
            }
            _ => panic!("Expected Initialize"),
        }
    }

    #[test]
    fn test_parse_submit_message_string_prompt() {
        let req = make_request(
            "submitMessage",
            json!({
                "prompt": "Hello, Claude!",
                "uuid": "abc-123"
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::SubmitMessage { prompt, uuid } => {
                assert_eq!(prompt, json!("Hello, Claude!"));
                assert_eq!(uuid, Some("abc-123".to_string()));
            }
            _ => panic!("Expected SubmitMessage"),
        }
    }

    #[test]
    fn test_parse_submit_message_without_uuid() {
        let req = make_request(
            "submitMessage",
            json!({
                "prompt": [{"type": "text", "text": "hi"}]
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::SubmitMessage { prompt, uuid } => {
                assert!(prompt.is_array());
                assert_eq!(uuid, None);
            }
            _ => panic!("Expected SubmitMessage"),
        }
    }

    #[test]
    fn test_parse_permission_response() {
        let req = make_request(
            "permissionResponse",
            json!({
                "tool_use_id": "tu_123",
                "decision": "allow",
                "rule": "Bash(git *)"
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::PermissionResponse {
                tool_use_id,
                decision,
                rule,
            } => {
                assert_eq!(tool_use_id, "tu_123");
                assert_eq!(decision, "allow");
                assert_eq!(rule, Some("Bash(git *)".to_string()));
            }
            _ => panic!("Expected PermissionResponse"),
        }
    }

    #[test]
    fn test_parse_permission_response_deny_no_rule() {
        let req = make_request(
            "permissionResponse",
            json!({
                "tool_use_id": "tu_456",
                "decision": "deny"
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::PermissionResponse {
                decision, rule, ..
            } => {
                assert_eq!(decision, "deny");
                assert_eq!(rule, None);
            }
            _ => panic!("Expected PermissionResponse"),
        }
    }

    #[test]
    fn test_parse_abort() {
        let req = make_request("abort", json!(null));
        let method = parse_client_method(&req).unwrap();
        assert_eq!(method, ClientMethod::Abort);
    }

    #[test]
    fn test_parse_update_settings() {
        let req = make_request(
            "updateSettings",
            json!({
                "settings": {"model": "claude-opus", "verbose": false}
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::UpdateSettings { settings } => {
                assert_eq!(settings, json!({"model": "claude-opus", "verbose": false}));
            }
            _ => panic!("Expected UpdateSettings"),
        }
    }

    #[test]
    fn test_parse_shutdown() {
        let req = make_request("shutdown", json!(null));
        let method = parse_client_method(&req).unwrap();
        assert_eq!(method, ClientMethod::Shutdown);
    }

    // --- Error cases ---

    #[test]
    fn test_parse_unknown_method() {
        let req = make_request("nonExistentMethod", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        match err {
            RouterError::UnknownMethod(m) => assert_eq!(m, "nonExistentMethod"),
            _ => panic!("Expected UnknownMethod, got {:?}", err),
        }
    }

    #[test]
    fn test_parse_invalid_params_missing_required_field() {
        // initialize requires "cwd" and "settings"
        let req = make_request("initialize", json!({"model": "test"}));
        let err = parse_client_method(&req).unwrap_err();
        match err {
            RouterError::InvalidParams(msg) => {
                assert!(msg.contains("cwd") || msg.contains("missing field"),
                    "Error should mention missing field, got: {}", msg);
            }
            _ => panic!("Expected InvalidParams, got {:?}", err),
        }
    }

    #[test]
    fn test_parse_invalid_params_wrong_type() {
        // cwd should be a string/path, not a number
        let req = make_request(
            "initialize",
            json!({"cwd": 42, "settings": {}}),
        );
        let err = parse_client_method(&req).unwrap_err();
        match err {
            RouterError::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams, got {:?}", err),
        }
    }

    #[test]
    fn test_parse_permission_response_missing_required() {
        // Missing tool_use_id and decision
        let req = make_request("permissionResponse", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        match err {
            RouterError::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams, got {:?}", err),
        }
    }

    // --- RouterError display ---

    #[test]
    fn test_router_error_display() {
        let err = RouterError::UnknownMethod("foo".to_string());
        assert_eq!(err.to_string(), "Unknown method: foo");

        let err = RouterError::InvalidParams("bad field".to_string());
        assert_eq!(err.to_string(), "Invalid params: bad field");
    }

    #[test]
    fn test_parse_compact() {
        let req = make_request("compact", json!(null));
        let method = parse_client_method(&req).unwrap();
        assert_eq!(method, ClientMethod::Compact);
    }

    #[test]
    fn test_parse_switch_model() {
        let req = make_request(
            "switchModel",
            json!({ "model": "claude-opus-4-20250514" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::SwitchModel { model } => {
                assert_eq!(model, "claude-opus-4-20250514");
            }
            _ => panic!("Expected SwitchModel, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_switch_model_missing_model() {
        let req = make_request("switchModel", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        match err {
            RouterError::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams, got {:?}", err),
        }
    }

    #[test]
    fn test_parse_git_diff() {
        let req = make_request("gitDiff", json!(null));
        let method = parse_client_method(&req).unwrap();
        assert_eq!(method, ClientMethod::GitDiff);
    }

    #[test]
    fn test_parse_git_commit() {
        let req = make_request(
            "gitCommit",
            json!({ "message": "feat: add git integration" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::GitCommit { message } => {
                assert_eq!(message, "feat: add git integration");
            }
            _ => panic!("Expected GitCommit, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_git_commit_missing_message() {
        let req = make_request("gitCommit", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        match err {
            RouterError::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams, got {:?}", err),
        }
    }

    #[test]
    fn test_parse_git_status() {
        let req = make_request("gitStatus", json!(null));
        let method = parse_client_method(&req).unwrap();
        assert_eq!(method, ClientMethod::GitStatus);
    }

    // --- Task management RPC tests ---

    #[test]
    fn test_parse_task_create() {
        let req = make_request(
            "taskCreate",
            json!({ "description": "Refactor auth module", "prompt": "Refactor the auth module" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TaskCreate { description, prompt } => {
                assert_eq!(description, "Refactor auth module");
                assert_eq!(prompt, "Refactor the auth module");
            }
            _ => panic!("Expected TaskCreate, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_task_create_missing_fields() {
        let req = make_request("taskCreate", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        assert!(matches!(err, RouterError::InvalidParams(_)));
    }

    #[test]
    fn test_parse_task_list() {
        let req = make_request("taskList", json!(null));
        let method = parse_client_method(&req).unwrap();
        assert_eq!(method, ClientMethod::TaskList);
    }

    #[test]
    fn test_parse_task_status() {
        let req = make_request(
            "taskStatus",
            json!({ "task_id": "abc12345" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TaskStatus { task_id } => {
                assert_eq!(task_id, "abc12345");
            }
            _ => panic!("Expected TaskStatus, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_task_stop() {
        let req = make_request(
            "taskStop",
            json!({ "task_id": "abc12345" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TaskStop { task_id } => {
                assert_eq!(task_id, "abc12345");
            }
            _ => panic!("Expected TaskStop, got {:?}", method),
        }
    }
}
