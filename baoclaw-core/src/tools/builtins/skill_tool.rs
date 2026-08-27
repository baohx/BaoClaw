use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::fs;

use crate::discovery::skills::discover_skills;
use crate::tools::trait_def::*;

const MAX_SKILL_BYTES: u64 = 256 * 1024;

/// SkillTool — load skill content on demand at runtime.
///
/// Mirrors Claude Code's Skill tool: the agent invokes it with a skill name,
/// and the tool returns the full SKILL.md content so the agent can follow it.
/// This avoids pre-loading all skills into the system prompt (which wastes
/// context window), instead loading only what's needed when it's needed.
pub struct SkillTool {
    cwd: PathBuf,
}

impl SkillTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    /// Find and read a specific skill by name.
    /// Searches user (~/.baoclaw/skills/) then project (<cwd>/.baoclaw/skills/).
    async fn find_skill(&self, name: &str) -> Option<(String, String)> {
        // Normalize: strip leading slash
        let name = name.trim_start_matches('/');
        if name.is_empty() || name.contains("..") || name.contains('/') || name.contains('\\') {
            return None;
        }

        let search_dirs: Vec<(PathBuf, &str)> = {
            let mut dirs = Vec::new();
            // User-level skills
            if let Ok(home) = std::env::var("HOME") {
                dirs.push((PathBuf::from(&home).join(".baoclaw").join("skills"), "user"));
            }
            // Project-level skills
            dirs.push((self.cwd.join(".baoclaw").join("skills"), "project"));
            dirs
        };

        for (dir, source) in &search_dirs {
            // Try directory format: <name>/SKILL.md
            let skill_file = dir.join(name).join("SKILL.md");
            if is_safe_skill_path(&skill_file, dir, true) {
                if let Ok(content) = fs::read_to_string(&skill_file).await {
                    if content.len() as u64 > MAX_SKILL_BYTES {
                        return None;
                    }
                    return Some((content, source.to_string()));
                }
            }
            // Try flat file format: <name>.md
            let flat_file = dir.join(format!("{}.md", name));
            if is_safe_skill_path(&flat_file, dir, false) {
                if let Ok(content) = fs::read_to_string(&flat_file).await {
                    if content.len() as u64 > MAX_SKILL_BYTES {
                        return None;
                    }
                    return Some((content, source.to_string()));
                }
            }
        }

        None
    }
}

fn is_safe_skill_path(
    path: &std::path::Path,
    root: &std::path::Path,
    directory_format: bool,
) -> bool {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() => {}
        _ => return false,
    }
    let target = match path.canonicalize() {
        Ok(target) => target,
        Err(_) => return false,
    };
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(_) => return false,
    };
    target.starts_with(&root)
        && (!directory_format || path.parent().is_some_and(|parent| parent.starts_with(root)))
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["skill", "LoadSkill"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "skill": {
                    "type": "string",
                    "description": "The skill name to load. E.g., \"test-driven-development\", \"using-superpowers\", \"brainstorming\". Use \"__list__\" to see all available skills."
                }
            })),
            required: Some(vec!["skill".to_string()]),
            description: Some("Load a skill's full instructions by name. Use this BEFORE starting any task — if a skill might apply, check it first. Skills give you detailed workflows for specific task types.".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn prompt(&self) -> String {
        "Load a skill's full instructions. Use whenever a skill might apply to your task — check for matching skills before starting work. Common skills include brainstorming, test-driven-development, systematic-debugging, writing-plans, executing-plans, subagent-driven-development, using-git-worktrees, and many more. Use '__list__' to see all available skills.".to_string()
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        match input.get("skill").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => ValidationResult::Ok,
            _ => ValidationResult::Invalid {
                message: "Missing or empty 'skill' field".to_string(),
                code: None,
            },
        }
    }

    async fn check_permissions(
        &self,
        _input: &Value,
        _context: &ToolContext,
    ) -> ToolPermissionCheckResult {
        // Skill tool is always safe — it only reads files
        ToolPermissionCheckResult::Allow {
            updated_input: _input.clone(),
        }
    }

    async fn call(
        &self,
        input: Value,
        _context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let skill_name = input.get("skill").and_then(|v| v.as_str()).unwrap_or("");

        // Special: list all available skills
        if skill_name == "__list__" {
            let skills = discover_skills(&self.cwd).await;
            let mut lines = Vec::new();
            lines.push(format!("# Available Skills ({})\n", skills.len()));
            for s in &skills {
                let desc = s.description.as_deref().unwrap_or("(no description)");
                lines.push(format!("- **{}** [{}]: {}", s.name, s.source, desc));
            }
            let text = lines.join("\n");
            return Ok(ToolResult {
                data: json!({
                    "success": true,
                    "skill_name": "__list__",
                    "content": text,
                    "count": skills.len(),
                }),
                is_error: false,
            });
        }

        // Load specific skill
        match self.find_skill(skill_name).await {
            Some((content, source)) => Ok(ToolResult {
                data: json!({
                    "success": true,
                    "skill_name": skill_name,
                    "source": source,
                    "content": content,
                }),
                is_error: false,
            }),
            None => Ok(ToolResult {
                data: json!({
                    "success": false,
                    "skill_name": skill_name,
                    "error": format!("Skill '{}' not found. Use Skill(skill: \"__list__\") to see available skills.", skill_name),
                }),
                is_error: true,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn test_skill_list() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp
            .path()
            .join(".baoclaw")
            .join("skills")
            .join("test-skill");
        std::fs::create_dir_all(&skills_dir).unwrap();
        let mut f = std::fs::File::create(skills_dir.join("SKILL.md")).unwrap();
        writeln!(
            f,
            "---\ndescription: Test skill for unit tests\n---\n\n# Test Skill\n\nDo the thing."
        )
        .unwrap();

        // Use the tmp dir as cwd to test project-level skill discovery
        let tool = SkillTool::new(tmp.path().to_path_buf());
        let result = tool.find_skill("test-skill").await;
        assert!(result.is_some());
        let (content, _) = result.unwrap();
        assert!(content.contains("Test Skill"));
        assert!(content.contains("Do the thing"));
    }

    #[tokio::test]
    async fn test_skill_not_found() {
        let tool = SkillTool::new(PathBuf::from("/tmp"));
        // Search for a skill name that is extremely unlikely to exist
        let result = tool.find_skill("zzz-nonexistent-skill-12345").await;
        assert!(result.is_none());
    }
}
