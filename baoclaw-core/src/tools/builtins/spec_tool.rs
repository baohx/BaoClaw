use async_trait::async_trait;
use serde_json::Value;

use crate::engine::spec_engine::{
    SpecEngine, SpecType, SpecWorkflow, TaskStatus,
};
use crate::tools::trait_def::{
    JsonSchema, ProgressSender, Tool, ToolContext, ToolError, ToolResult,
};

pub struct SpecTool;

impl SpecTool {
    pub fn new() -> Self {
        Self {}
    }

    fn get_or_init_engine(&self, cwd: &std::path::Path) -> SpecEngine {
        SpecEngine::new(cwd.to_path_buf())
    }

    fn error_result(msg: &str) -> ToolResult {
        ToolResult {
            data: serde_json::json!({"error": msg}),
            is_error: true,
        }
    }

    fn ok_result(data: Value) -> ToolResult {
        ToolResult { data, is_error: false }
    }
}

#[async_trait]
impl Tool for SpecTool {
    fn name(&self) -> &str {
        "Spec"
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(serde_json::json!({
                "action": {
                    "type": "string",
                    "enum": ["new", "list", "show", "status", "update_task", "read_doc", "write_doc"],
                    "description": "The spec action to perform"
                },
                "feature_name": {
                    "type": "string",
                    "description": "The feature name (kebab-case)"
                },
                "workflow": {
                    "type": "string",
                    "enum": ["requirements", "design"],
                    "description": "Workflow type for 'new' action"
                },
                "spec_type": {
                    "type": "string",
                    "enum": ["feature", "bugfix"],
                    "description": "Spec type for 'new' action"
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID for 'update_task' action"
                },
                "status": {
                    "type": "string",
                    "enum": ["not_started", "in_progress", "completed"],
                    "description": "New status for 'update_task' action"
                },
                "phase": {
                    "type": "string",
                    "enum": ["requirements", "design", "tasks"],
                    "description": "Phase for 'read_doc'/'write_doc' actions"
                },
                "content": {
                    "type": "string",
                    "description": "Content for 'write_doc' action"
                }
            })),
            required: Some(vec!["action".to_string()]),
            description: Some("Manage spec-driven development documents".to_string()),
        }
    }

    fn is_read_only(&self, input: &Value) -> bool {
        matches!(
            input.get("action").and_then(|v| v.as_str()),
            Some("list") | Some("show") | Some("status") | Some("read_doc")
        )
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let engine = self.get_or_init_engine(&context.cwd);

        let result = match action {
            "new" => {
                let feature_name = input.get("feature_name").and_then(|v| v.as_str()).unwrap_or("");
                if feature_name.is_empty() {
                    return Ok(Self::error_result("feature_name is required"));
                }
                let workflow = match input.get("workflow").and_then(|v| v.as_str()) {
                    Some("design") => SpecWorkflow::DesignFirst,
                    _ => SpecWorkflow::RequirementsFirst,
                };
                let spec_type = match input.get("spec_type").and_then(|v| v.as_str()) {
                    Some("bugfix") => SpecType::Bugfix,
                    _ => SpecType::Feature,
                };
                match engine.create_spec(feature_name, workflow, spec_type) {
                    Ok(config) => Self::ok_result(serde_json::json!({
                        "status": "created",
                        "feature_name": feature_name,
                        "config": serde_json::to_value(&config).unwrap_or_default()
                    })),
                    Err(e) => Self::error_result(&e.to_string()),
                }
            }
            "list" => match engine.list_specs() {
                Ok(specs) => Self::ok_result(serde_json::json!({
                    "specs": serde_json::to_value(&specs).unwrap_or_default()
                })),
                Err(e) => Self::error_result(&e.to_string()),
            },
            "show" => {
                let feature_name = input.get("feature_name").and_then(|v| v.as_str()).unwrap_or("");
                match engine.get_spec(feature_name) {
                    Ok(summary) => Self::ok_result(serde_json::to_value(&summary).unwrap_or_default()),
                    Err(e) => Self::error_result(&e.to_string()),
                }
            }
            "status" => {
                let feature_name = input.get("feature_name").and_then(|v| v.as_str()).unwrap_or("");
                match engine.get_status(feature_name) {
                    Ok(progress) => Self::ok_result(serde_json::to_value(&progress).unwrap_or_default()),
                    Err(e) => Self::error_result(&e.to_string()),
                }
            }
            "update_task" => {
                let feature_name = input.get("feature_name").and_then(|v| v.as_str()).unwrap_or("");
                let task_id = input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                let status = match input.get("status").and_then(|v| v.as_str()) {
                    Some("in_progress") => TaskStatus::InProgress,
                    Some("completed") => TaskStatus::Completed,
                    _ => TaskStatus::NotStarted,
                };
                match engine.update_task_status(feature_name, task_id, status) {
                    Ok(()) => Self::ok_result(serde_json::json!({"status": "updated"})),
                    Err(e) => Self::error_result(&e.to_string()),
                }
            }
            "read_doc" => {
                let feature_name = input.get("feature_name").and_then(|v| v.as_str()).unwrap_or("");
                let phase = input.get("phase").and_then(|v| v.as_str()).unwrap_or("");
                match engine.read_phase_doc(feature_name, phase) {
                    Ok(content) => Self::ok_result(serde_json::json!({"content": content})),
                    Err(e) => Self::error_result(&e.to_string()),
                }
            }
            "write_doc" => {
                let feature_name = input.get("feature_name").and_then(|v| v.as_str()).unwrap_or("");
                let phase = input.get("phase").and_then(|v| v.as_str()).unwrap_or("");
                let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
                match engine.write_phase_doc(feature_name, phase, content) {
                    Ok(()) => Self::ok_result(serde_json::json!({"status": "written"})),
                    Err(e) => Self::error_result(&e.to_string()),
                }
            }
            _ => Self::error_result(&format!("Unknown action: {}", action)),
        };

        Ok(result)
    }

    fn prompt(&self) -> String {
        "Spec — Manage spec-driven development documents. Create, list, show, and update specs \
         (requirements → design → tasks workflow). Use to track feature development progress \
         and update task statuses.".to_string()
    }
}
