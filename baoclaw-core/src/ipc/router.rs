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
        #[serde(default)]
        shared_session_id: Option<String>,
    },
    #[serde(rename = "submitMessage")]
    SubmitMessage {
        prompt: Value,
        uuid: Option<String>,
        #[serde(default)]
        attachments: Option<Vec<Value>>,
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
    #[serde(rename = "memoryList")]
    MemoryList,
    #[serde(rename = "memoryAdd")]
    MemoryAdd {
        content: String,
        #[serde(default = "default_category")]
        category: String,
    },
    #[serde(rename = "memoryDelete")]
    MemoryDelete {
        id: String,
    },
    #[serde(rename = "memoryClear")]
    MemoryClear,
    #[serde(rename = "memoryStats")]
    MemoryStats,
    #[serde(rename = "memoryArchive")]
    MemoryArchive {
        id: String,
    },
    #[serde(rename = "memoryRestore")]
    MemoryRestore {
        id: String,
    },
    #[serde(rename = "memoryArchiveList")]
    MemoryArchiveList,
    #[serde(rename = "memoryCleanup")]
    MemoryCleanup,
    #[serde(rename = "switchCwd")]
    SwitchCwd {
        cwd: PathBuf,
    },
    #[serde(rename = "cronAdd")]
    CronAdd {
        name: String,
        prompt: String,
        schedule: String,
        #[serde(default)]
        cwd: Option<String>,
    },
    #[serde(rename = "cronRemove")]
    CronRemove { id: String },
    #[serde(rename = "cronToggle")]
    CronToggle { id: String },
    #[serde(rename = "cronList")]
    CronList,
    #[serde(rename = "projectsList")]
    ProjectsList,
    #[serde(rename = "projectsSwitch")]
    ProjectsSwitch { id_prefix: String },
    #[serde(rename = "projectsNew")]
    ProjectsNew {
        cwd: String,
        #[serde(default)]
        description: Option<String>,
    },
    #[serde(rename = "projectsUpdateDesc")]
    ProjectsUpdateDesc {
        id_prefix: String,
        description: String,
    },
    #[serde(rename = "talkTail")]
    TalkTail {
        #[serde(default = "default_tail_count")]
        count: usize,
    },
    #[serde(rename = "searchHistory")]
    SearchHistory {
        query: String,
        #[serde(default = "default_tail_count")]
        max_results: usize,
    },
    #[serde(rename = "docUpload")]
    DocUpload {
        file_path: String,
    },
    #[serde(rename = "export")]
    Export {
        #[serde(default)]
        output_path: Option<String>,
    },
    #[serde(rename = "specNew")]
    SpecNew {
        feature_name: String,
        #[serde(default)]
        workflow: Option<String>,
        #[serde(default)]
        spec_type: Option<String>,
    },
    #[serde(rename = "specList")]
    SpecList,
    #[serde(rename = "specShow")]
    SpecShow { feature_name: String },
    #[serde(rename = "specStatus")]
    SpecStatus { feature_name: String },
    #[serde(rename = "specRun")]
    SpecRun {
        feature_name: String,
        #[serde(default)]
        task_id: Option<String>,
    },
    #[serde(rename = "specEdit")]
    SpecEdit {
        feature_name: String,
        phase: String,
    },
    #[serde(rename = "hooksList")]
    HooksList,
    #[serde(rename = "hooksAdd")]
    HooksAdd {
        id: String,
        name: String,
        trigger: String,
        #[serde(default)]
        filter: Option<Value>,
        action: Value,
        #[serde(default)]
        enabled: bool,
        #[serde(default)]
        priority: i32,
    },
    #[serde(rename = "hooksToggle")]
    HooksToggle {
        id: String,
    },
    #[serde(rename = "hooksRemove")]
    HooksRemove {
        id: String,
    },

    // ── Team Management RPC ──
    #[serde(rename = "teamSpawn")]
    TeamSpawn {
        /// Number of parallel agents to create (for parallel mode).
        #[serde(default)]
        count: Option<usize>,
        /// Execution mode: "parallel", "sequence", or "dag".
        mode: String,
        /// The task prompt for the team.
        task: String,
        /// Optional team policy for budget and permissions.
        #[serde(default)]
        policy: Option<Value>,
    },
    #[serde(rename = "teamList")]
    TeamList,
    #[serde(rename = "teamStatus")]
    TeamStatus {
        team_id: String,
    },
    #[serde(rename = "teamResults")]
    TeamResults {
        team_id: String,
    },
    #[serde(rename = "teamAbort")]
    TeamAbort {
        team_id: String,
    },
    #[serde(rename = "teamExecute")]
    TeamExecute {
        team_id: String,
    },

    // ── Template Engine RPC ──
    #[serde(rename = "templateList")]
    TemplateList,
    #[serde(rename = "templateCreate")]
    TemplateCreate {
        json: String,
    },
    #[serde(rename = "templateDelete")]
    TemplateDelete {
        name: String,
    },
    #[serde(rename = "templateExport")]
    TemplateExport {
        name: String,
    },
    #[serde(rename = "templateImport")]
    TemplateImport {
        url: String,
    },

    // ── Git Integration RPC ──
    #[serde(rename = "gitPrCreate")]
    GitPrCreate {
        title: String,
        body: String,
        base: String,
        head: String,
    },
    #[serde(rename = "gitPrList")]
    GitPrList,
    #[serde(rename = "gitBranchList")]
    GitBranchList,
    #[serde(rename = "gitBranchCreate")]
    GitBranchCreate {
        name: String,
    },
    #[serde(rename = "gitConflictCheck")]
    GitConflictCheck,
    #[serde(rename = "gitCommitAmend")]
    GitCommitAmend,
    #[serde(rename = "gitUndo")]
    GitUndo,

    // ── Model Router RPC ──
    #[serde(rename = "modelList")]
    ModelList,
    #[serde(rename = "modelRoute")]
    ModelRoute {
        task: String,
    },
    #[serde(rename = "modelBudget")]
    ModelBudget,
    #[serde(rename = "modelStats")]
    ModelStats,

    // ── Telemetry RPC ──
    #[serde(rename = "telemetryStats")]
    TelemetryStats,
    #[serde(rename = "telemetryTrends")]
    TelemetryTrends {
        days: u32,
    },
    #[serde(rename = "telemetryExport")]
    TelemetryExport {
        format: String,
    },

    // ── Permission Gate RPC ──
    #[serde(rename = "permissionStatus")]
    PermissionStatus,
    #[serde(rename = "permissionGrant")]
    PermissionGrant {
        tool: String,
        action: String,
        target: String,
        permanent: bool,
    },
    #[serde(rename = "permissionRevoke")]
    PermissionRevoke {
        tool: String,
        action: String,
        target: String,
    },

    // ── Permission Manager RPC (rule-based) ──
    #[serde(rename = "permissions.info")]
    PermissionsInfo,
    #[serde(rename = "permissions.addRule")]
    PermissionsAddRule {
        category: String,
        tool_name: String,
        #[serde(default)]
        rule_content: Option<String>,
    },
    #[serde(rename = "permissions.removeRule")]
    PermissionsRemoveRule {
        category: String,
        tool_name: String,
        #[serde(default)]
        rule_content: Option<String>,
    },
    #[serde(rename = "permissions.setMode")]
    PermissionsSetMode {
        mode: String,
    },

    // ── Session Info / Token / Cost RPC (P2-2) ──
    #[serde(rename = "session.tokens")]
    SessionTokens,
    #[serde(rename = "session.cost")]
    SessionCost,
    #[serde(rename = "session.info")]
    SessionInfo,
    #[serde(rename = "config.model")]
    ConfigModel,
    #[serde(rename = "config.show")]
    ConfigShow,
}

fn default_tail_count() -> usize { 10 }

fn default_category() -> String { "fact".to_string() }

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
            ClientMethod::SubmitMessage { prompt, uuid, .. } => {
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
            ClientMethod::SubmitMessage { prompt, uuid, .. } => {
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

    #[test]
    fn test_parse_doc_upload() {
        let req = make_request(
            "docUpload",
            json!({ "file_path": "/home/user/report.pdf" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::DocUpload { file_path } => {
                assert_eq!(file_path, "/home/user/report.pdf");
            }
            _ => panic!("Expected DocUpload, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_doc_upload_missing_path() {
        let req = make_request("docUpload", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        assert!(matches!(err, RouterError::InvalidParams(_)));
    }

    // --- Hooks management RPC tests ---

    #[test]
    fn test_parse_hooks_list() {
        let req = make_request("hooksList", json!(null));
        let method = parse_client_method(&req).unwrap();
        assert_eq!(method, ClientMethod::HooksList);
    }

    #[test]
    fn test_parse_hooks_add() {
        let req = make_request(
            "hooksAdd",
            json!({
                "id": "auto-lint",
                "name": "Auto Lint on Save",
                "trigger": "file_edited",
                "filter": { "file_pattern": "*.ts" },
                "action": { "type": "run_command", "command": "npm run lint" },
                "enabled": true,
                "priority": 100
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::HooksAdd { id, name, trigger, filter, action, enabled, priority } => {
                assert_eq!(id, "auto-lint");
                assert_eq!(name, "Auto Lint on Save");
                assert_eq!(trigger, "file_edited");
                assert!(filter.is_some());
                assert_eq!(action["type"], "run_command");
                assert!(enabled);
                assert_eq!(priority, 100);
            }
            _ => panic!("Expected HooksAdd, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_hooks_add_minimal() {
        let req = make_request(
            "hooksAdd",
            json!({
                "id": "test-hook",
                "name": "Test Hook",
                "trigger": "file_created",
                "action": { "type": "ask_agent", "prompt": "Review this file" }
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::HooksAdd { id, name, trigger, filter, action, enabled, priority } => {
                assert_eq!(id, "test-hook");
                assert_eq!(name, "Test Hook");
                assert_eq!(trigger, "file_created");
                assert!(filter.is_none());
                assert_eq!(action["type"], "ask_agent");
                assert!(!enabled); // default false when not specified
                assert_eq!(priority, 0); // default 0
            }
            _ => panic!("Expected HooksAdd, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_hooks_add_missing_required() {
        let req = make_request("hooksAdd", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        assert!(matches!(err, RouterError::InvalidParams(_)));
    }

    #[test]
    fn test_parse_hooks_toggle() {
        let req = make_request(
            "hooksToggle",
            json!({ "id": "auto-lint" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::HooksToggle { id } => {
                assert_eq!(id, "auto-lint");
            }
            _ => panic!("Expected HooksToggle, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_hooks_toggle_missing_id() {
        let req = make_request("hooksToggle", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        assert!(matches!(err, RouterError::InvalidParams(_)));
    }

    #[test]
    fn test_parse_hooks_remove() {
        let req = make_request(
            "hooksRemove",
            json!({ "id": "auto-lint" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::HooksRemove { id } => {
                assert_eq!(id, "auto-lint");
            }
            _ => panic!("Expected HooksRemove, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_hooks_remove_missing_id() {
        let req = make_request("hooksRemove", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        assert!(matches!(err, RouterError::InvalidParams(_)));
    }

    // --- Team Management RPC tests ---

    #[test]
    fn test_parse_team_spawn_parallel() {
        let req = make_request(
            "teamSpawn",
            json!({
                "count": 3,
                "mode": "parallel",
                "task": "Analyze the codebase"
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TeamSpawn { count, mode, task, policy } => {
                assert_eq!(count, Some(3));
                assert_eq!(mode, "parallel");
                assert_eq!(task, "Analyze the codebase");
                assert!(policy.is_none());
            }
            _ => panic!("Expected TeamSpawn, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_team_spawn_sequence() {
        let req = make_request(
            "teamSpawn",
            json!({
                "mode": "sequence",
                "task": "First analyze, then implement"
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TeamSpawn { count, mode, task, .. } => {
                assert_eq!(count, None);
                assert_eq!(mode, "sequence");
                assert_eq!(task, "First analyze, then implement");
            }
            _ => panic!("Expected TeamSpawn, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_team_spawn_with_policy() {
        let req = make_request(
            "teamSpawn",
            json!({
                "mode": "parallel",
                "task": "Test task",
                "policy": {
                    "max_turns_per_agent": 5,
                    "max_cost_per_agent": 1.0
                }
            }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TeamSpawn { policy, .. } => {
                assert!(policy.is_some());
                let p = policy.unwrap();
                assert_eq!(p["max_turns_per_agent"], 5);
                assert_eq!(p["max_cost_per_agent"], 1.0);
            }
            _ => panic!("Expected TeamSpawn, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_team_spawn_missing_required() {
        let req = make_request("teamSpawn", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        assert!(matches!(err, RouterError::InvalidParams(_)));
    }

    #[test]
    fn test_parse_team_list() {
        let req = make_request("teamList", json!(null));
        let method = parse_client_method(&req).unwrap();
        assert_eq!(method, ClientMethod::TeamList);
    }

    #[test]
    fn test_parse_team_status() {
        let req = make_request(
            "teamStatus",
            json!({ "team_id": "team-123" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TeamStatus { team_id } => {
                assert_eq!(team_id, "team-123");
            }
            _ => panic!("Expected TeamStatus, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_team_status_missing_id() {
        let req = make_request("teamStatus", json!({}));
        let err = parse_client_method(&req).unwrap_err();
        assert!(matches!(err, RouterError::InvalidParams(_)));
    }

    #[test]
    fn test_parse_team_results() {
        let req = make_request(
            "teamResults",
            json!({ "team_id": "team-456" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TeamResults { team_id } => {
                assert_eq!(team_id, "team-456");
            }
            _ => panic!("Expected TeamResults, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_team_abort() {
        let req = make_request(
            "teamAbort",
            json!({ "team_id": "team-789" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TeamAbort { team_id } => {
                assert_eq!(team_id, "team-789");
            }
            _ => panic!("Expected TeamAbort, got {:?}", method),
        }
    }

    #[test]
    fn test_parse_team_execute() {
        let req = make_request(
            "teamExecute",
            json!({ "team_id": "team-abc" }),
        );
        let method = parse_client_method(&req).unwrap();
        match method {
            ClientMethod::TeamExecute { team_id } => {
                assert_eq!(team_id, "team-abc");
            }
            _ => panic!("Expected TeamExecute, got {:?}", method),
        }
    }
}
