//! Evolve tool — allows the agent to create, improve, and manage skills autonomously.
//!
//! Operations:
//! - create_skill: Write a new skill from observed patterns
//! - improve_skill: Refine an existing skill based on usage feedback
//! - list_candidates: Show auto-extracted skill candidates
//! - promote: Promote a candidate to a real skill
//! - export_training: Export trajectory data for RLHF fine-tuning

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::engine::evolution::EvolutionEngine;
use crate::tools::trait_def::*;

pub struct EvolveTool {
    evolution: Arc<EvolutionEngine>,
}

impl EvolveTool {
    pub fn new(evolution: Arc<EvolutionEngine>) -> Self {
        Self { evolution }
    }
}

#[async_trait]
impl Tool for EvolveTool {
    fn name(&self) -> &str {
        "Evolve"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["evolve", "self_improve"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "operation": {
                    "type": "string",
                    "enum": ["create_skill", "improve_skill", "list_candidates", "promote", "export_training"],
                    "description": "The evolution operation to perform"
                },
                "skill_name": {
                    "type": "string",
                    "description": "Name of the skill (for create/improve/promote)"
                },
                "content": {
                    "type": "string",
                    "description": "Markdown content of the skill (for create/improve)"
                },
                "reason": {
                    "type": "string",
                    "description": "Why this skill is being created or how it's being improved"
                }
            })),
            required: Some(vec!["operation".to_string()]),
            description: Some("Create, improve, and manage skills for self-evolution".to_string()),
        }
    }

    fn prompt(&self) -> String {
        r#"Self-evolution tool. Use this to make yourself better over time:

- create_skill: When you notice a repeating pattern in user requests, create a reusable skill.
  Skills are markdown files that describe a procedure. Include: trigger conditions, step-by-step
  instructions, examples, and edge cases. Good skills save the user from repeating instructions.

- improve_skill: When a skill works but could be better, update it with lessons learned.
  Include what changed and why.

- list_candidates: Show auto-extracted skill candidates from recent successful interactions.

- promote: Promote a candidate to a real skill (provide refined content).

- export_training: Export interaction trajectories as RLHF training data for model fine-tuning.

Guidelines for skill creation:
1. Only create skills for patterns you've seen at least 2-3 times
2. Skills should be specific enough to be useful, general enough to be reusable
3. Include the user's preferences and style in the skill
4. Update skills when you learn something new about the user's workflow"#.to_string()
    }

    fn is_read_only(&self, input: &Value) -> bool {
        let op = input.get("operation").and_then(|v| v.as_str()).unwrap_or("");
        matches!(op, "list_candidates" | "export_training")
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let operation = input.get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'operation'".to_string()))?;

        match operation {
            "create_skill" => {
                let name = input.get("skill_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'skill_name'".to_string()))?;
                let content = input.get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'content'".to_string()))?;
                let reason = input.get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Auto-created by evolution engine");

                let skills_dir = context.cwd.join(".baoclaw").join("skills");
                let _ = std::fs::create_dir_all(&skills_dir);
                let skill_path = skills_dir.join(format!("{}.md", name));

                // Add metadata header
                let full_content = format!(
                    "---\ndescription: {}\ncreated_by: evolution\ncreated_at: {}\nversion: 1\n---\n\n{}",
                    reason,
                    chrono::Utc::now().to_rfc3339(),
                    content
                );

                std::fs::write(&skill_path, &full_content)
                    .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write skill: {}", e)))?;

                Ok(ToolResult {
                    data: json!({
                        "created": true,
                        "path": skill_path.display().to_string(),
                        "name": name,
                    }),
                    is_error: false,
                })
            }

            "improve_skill" => {
                let name = input.get("skill_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'skill_name'".to_string()))?;
                let content = input.get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'content'".to_string()))?;
                let reason = input.get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Improved based on usage");

                let skill_path = context.cwd.join(".baoclaw").join("skills").join(format!("{}.md", name));

                // Read existing to get version
                let version = if let Ok(existing) = std::fs::read_to_string(&skill_path) {
                    existing.lines()
                        .find(|l| l.starts_with("version:"))
                        .and_then(|l| l.split(':').nth(1))
                        .and_then(|v| v.trim().parse::<u32>().ok())
                        .unwrap_or(0) + 1
                } else {
                    1
                };

                let full_content = format!(
                    "---\ndescription: {}\nupdated_by: evolution\nupdated_at: {}\nversion: {}\n---\n\n{}",
                    reason,
                    chrono::Utc::now().to_rfc3339(),
                    version,
                    content
                );

                std::fs::write(&skill_path, &full_content)
                    .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write skill: {}", e)))?;

                Ok(ToolResult {
                    data: json!({
                        "improved": true,
                        "path": skill_path.display().to_string(),
                        "version": version,
                    }),
                    is_error: false,
                })
            }

            "list_candidates" => {
                let candidates = self.evolution.list_candidates().await;
                Ok(ToolResult {
                    data: json!({
                        "candidates": candidates,
                        "count": candidates.len(),
                    }),
                    is_error: false,
                })
            }

            "promote" => {
                let name = input.get("skill_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'skill_name'".to_string()))?;
                let content = input.get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'content'".to_string()))?;

                match self.evolution.promote_skill(&context.cwd, name, content).await {
                    Ok(path) => Ok(ToolResult {
                        data: json!({"promoted": true, "path": path}),
                        is_error: false,
                    }),
                    Err(e) => Ok(ToolResult {
                        data: json!({"error": e}),
                        is_error: true,
                    }),
                }
            }

            "export_training" => {
                let data = self.evolution.export_training_data().await;
                let count = data.len();

                // Also write to a file for easy access
                let dir = context.cwd.join(".baoclaw").join(EVOLUTION_DIR);
                let _ = std::fs::create_dir_all(&dir);
                let export_path = dir.join("training_export.jsonl");
                if let Ok(mut f) = std::fs::File::create(&export_path) {
                    use std::io::Write;
                    for item in &data {
                        if let Ok(line) = serde_json::to_string(item) {
                            let _ = writeln!(f, "{}", line);
                        }
                    }
                }

                Ok(ToolResult {
                    data: json!({
                        "exported": count,
                        "path": export_path.display().to_string(),
                        "format": "jsonl",
                        "fields": ["prompt", "response", "rating", "tool_count", "duration_ms"],
                    }),
                    is_error: false,
                })
            }

            other => Err(ToolError::ExecutionFailed(format!("Unknown operation: {}", other))),
        }
    }
}

const EVOLUTION_DIR: &str = "evolution";
