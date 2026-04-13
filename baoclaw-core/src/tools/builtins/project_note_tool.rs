use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::trait_def::*;

/// Tool that allows the AI to automatically append project rules
/// and notes to BAOCLAW.md in the project directory.
pub struct ProjectNoteTool;

impl ProjectNoteTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for ProjectNoteTool {
    fn name(&self) -> &str { "ProjectNoteTool" }

    fn aliases(&self) -> Vec<&str> { vec!["ProjectNote", "SaveProjectRule"] }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "note": { "type": "string", "description": "The project rule or note to save" }
            })),
            required: Some(vec!["note".to_string()]),
            description: Some("Append a project rule or note to BAOCLAW.md".to_string()),
        }
    }

    fn prompt(&self) -> String {
        "Append a project-specific rule or note to BAOCLAW.md. Use this when you discover \
         project conventions, coding standards, build instructions, or other project-specific \
         information that should be remembered for this project. The note will be appended \
         to the project's BAOCLAW.md file and loaded into context for all future conversations \
         in this project directory."
            .to_string()
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let note = input.get("note").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'note'".into()))?;

        // Write to <cwd>/BAOCLAW.md (create if not exists)
        let baoclaw_path = context.cwd.join("BAOCLAW.md");
        let existing = tokio::fs::read_to_string(&baoclaw_path).await.unwrap_or_default();

        let new_content = if existing.is_empty() {
            format!("# Project Notes\n\n- {}\n", note)
        } else {
            format!("{}\n- {}\n", existing.trim_end(), note)
        };

        tokio::fs::write(&baoclaw_path, &new_content).await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write BAOCLAW.md: {}", e)))?;

        Ok(ToolResult {
            data: json!({ "saved": true, "path": baoclaw_path.to_string_lossy(), "note": note }),
            is_error: false,
        })
    }
}
