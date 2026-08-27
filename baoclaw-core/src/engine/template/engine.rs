//! Template engine — loads, parses, and executes workflow templates.
//!
//! Templates are loaded from `~/.baoclaw/templates/` directory as JSON files
//! plus 5 built-in templates embedded in the binary.

use std::collections::HashMap;
use std::path::PathBuf;

use super::builtins::builtin_templates;
use super::types::{Template, WorkflowAction, WorkflowStep};

/// Result of variable collection — either resolved or needs user input.
#[derive(Clone, Debug)]
pub enum VariableCollectResult {
    /// All required variables have been resolved.
    Resolved(HashMap<String, String>),
    /// Some variables need user input.
    NeedsInput {
        /// Variables awaiting input.
        prompts: Vec<VariablePrompt>,
        /// Currently resolved variables.
        resolved: HashMap<String, String>,
    },
}

/// A prompt for a missing variable.
#[derive(Clone, Debug)]
pub struct VariablePrompt {
    pub variable: String,
    pub prompt: String,
    pub default: Option<String>,
    pub required: bool,
    pub help: String,
}

/// The template engine manages template loading, matching, and variable resolution.
#[derive(Clone, Debug)]
pub struct TemplateEngine {
    /// User templates directory.
    templates_dir: PathBuf,
    /// Loaded templates (name → template).
    templates: HashMap<String, Template>,
}

impl TemplateEngine {
    /// Create a new engine with default templates directory.
    pub fn new() -> Self {
        let dir = default_templates_dir();
        let mut engine = Self {
            templates_dir: dir,
            templates: HashMap::new(),
        };
        // Load built-in templates
        for tpl in builtin_templates() {
            engine.templates.insert(tpl.name.clone(), tpl);
        }
        // Try to load user templates from disk
        engine.reload_user_templates();
        engine
    }

    /// Create a new engine with a custom templates directory (for testing).
    pub fn with_dir(dir: PathBuf) -> Self {
        let mut engine = Self {
            templates_dir: dir,
            templates: HashMap::new(),
        };
        // Load built-in templates
        for tpl in builtin_templates() {
            engine.templates.insert(tpl.name.clone(), tpl);
        }
        engine.reload_user_templates();
        engine
    }

    /// Reload user templates from disk.
    pub fn reload_user_templates(&mut self) {
        // Remove non-builtin templates before reloading
        self.templates.retain(|_, v| v.builtin);

        if let Ok(entries) = std::fs::read_dir(&self.templates_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(mut template) = serde_json::from_str::<Template>(&content) {
                            // Use filename stem as key
                            let name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            template.name = name.clone();
                            self.templates.insert(name, template);
                        }
                    }
                }
            }
        }
    }

    /// Find a template matching the given slash command.
    pub fn match_trigger(&self, command: &str) -> Option<&Template> {
        self.templates.values().find(|t| t.matches_trigger(command))
    }

    /// Find a template by name.
    pub fn find_by_name(&self, name: &str) -> Option<&Template> {
        self.templates.get(name)
    }

    /// List all loaded templates.
    pub fn list_all(&self) -> Vec<&Template> {
        let mut templates: Vec<&Template> = self.templates.values().collect();
        templates.sort_by(|a, b| a.name.cmp(&b.name));
        templates
    }

    /// List templates matching a search query.
    pub fn search(&self, query: &str) -> Vec<&Template> {
        let lower = query.to_lowercase();
        self.templates
            .values()
            .filter(|t| {
                t.name.to_lowercase().contains(&lower)
                    || t.description.to_lowercase().contains(&lower)
                    || t.tags.iter().any(|tag| tag.to_lowercase().contains(&lower))
            })
            .collect()
    }

    /// Collect variables for a template.
    /// Populates defaults and identifies missing required variables.
    pub fn collect_variables(
        &self,
        template: &Template,
        user_vars: HashMap<String, String>,
    ) -> VariableCollectResult {
        let mut resolved = HashMap::new();
        let mut prompts = Vec::new();

        for (key, var) in &template.variables {
            if let Some(val) = user_vars.get(key) {
                resolved.insert(key.clone(), val.clone());
            } else if !var.default.is_empty() {
                resolved.insert(key.clone(), var.default.clone());
            } else if var.required {
                prompts.push(VariablePrompt {
                    variable: key.clone(),
                    prompt: if var.prompt.is_empty() {
                        format!("Enter value for {}", key)
                    } else {
                        var.prompt.clone()
                    },
                    default: None,
                    required: true,
                    help: var.help.clone(),
                });
            }
        }

        if prompts.is_empty() {
            VariableCollectResult::Resolved(resolved)
        } else {
            VariableCollectResult::NeedsInput { prompts, resolved }
        }
    }

    /// Generate the augmented system prompt from the template.
    pub fn build_system_prompt(
        &self,
        template: &Template,
        vars: &HashMap<String, String>,
    ) -> String {
        if template.system_prompt_addon.is_empty() {
            return String::new();
        }
        template.substitute(&template.system_prompt_addon, vars)
    }

    /// Expand workflow steps with variable substitution.
    pub fn expand_steps(
        &self,
        template: &Template,
        vars: &HashMap<String, String>,
    ) -> Vec<WorkflowStep> {
        template
            .workflow
            .iter()
            .filter(|step| {
                if let Some(ref cond) = step.condition {
                    let expanded = template.substitute(cond, vars);
                    // Simple true/false evaluation
                    !matches!(
                        expanded.trim().to_lowercase().as_str(),
                        "false" | "0" | "no"
                    )
                } else {
                    true
                }
            })
            .map(|step| {
                let expanded_action = match &step.action {
                    WorkflowAction::Bash {
                        command,
                        capture_output,
                    } => WorkflowAction::Bash {
                        command: template.substitute(command, vars),
                        capture_output: *capture_output,
                    },
                    WorkflowAction::Analyze { prompt, files } => WorkflowAction::Analyze {
                        prompt: template.substitute(prompt, vars),
                        files: files.iter().map(|f| template.substitute(f, vars)).collect(),
                    },
                    WorkflowAction::Format { template: ref tpl } => WorkflowAction::Format {
                        template: template.substitute(tpl, vars),
                    },
                    WorkflowAction::Ask {
                        question,
                        variable,
                        default,
                    } => WorkflowAction::Ask {
                        question: template.substitute(question, vars),
                        variable: variable.clone(),
                        default: default.clone(),
                    },
                    WorkflowAction::Select {
                        question,
                        variable,
                        options,
                        default,
                    } => WorkflowAction::Select {
                        question: template.substitute(question, vars),
                        variable: variable.clone(),
                        options: options.clone(),
                        default: template.substitute(default, vars),
                    },
                    WorkflowAction::SubTemplate {
                        name,
                        variables: ref tvars,
                    } => {
                        let mut expanded_vars = HashMap::new();
                        for (k, v) in tvars {
                            expanded_vars.insert(k.clone(), template.substitute(v, vars));
                        }
                        WorkflowAction::SubTemplate {
                            name: template.substitute(name, vars),
                            variables: expanded_vars,
                        }
                    }
                };

                WorkflowStep {
                    step: template.substitute(&step.step, vars),
                    action: expanded_action,
                    condition: step
                        .condition
                        .as_ref()
                        .map(|c| template.substitute(c, vars)),
                }
            })
            .collect()
    }

    /// Create a new template and save it to disk.
    pub fn create_template(&mut self, template: &Template) -> Result<(), String> {
        if self.templates.contains_key(&template.name) {
            return Err(format!("Template '{}' already exists", template.name));
        }

        let path = self.template_path(&template.name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create templates dir: {}", e))?;
        }

        let json = serde_json::to_string_pretty(template)
            .map_err(|e| format!("Failed to serialize template: {}", e))?;
        std::fs::write(&path, json).map_err(|e| format!("Failed to write template: {}", e))?;

        self.templates
            .insert(template.name.clone(), template.clone());
        Ok(())
    }

    /// Update an existing template and save to disk.
    pub fn update_template(&mut self, name: &str, template: &Template) -> Result<(), String> {
        if !self.templates.contains_key(name) {
            return Err(format!("Template '{}' not found", name));
        }

        let existing = self.templates.get(name).unwrap();
        if existing.builtin {
            return Err(format!(
                "Cannot modify built-in template '{}'. Save as a new template with /template create",
                name
            ));
        }

        let path = self.template_path(name);
        let json = serde_json::to_string_pretty(template)
            .map_err(|e| format!("Failed to serialize template: {}", e))?;
        std::fs::write(&path, json).map_err(|e| format!("Failed to write template: {}", e))?;

        self.templates.insert(name.to_string(), template.clone());
        Ok(())
    }

    /// Delete a non-builtin template.
    pub fn delete_template(&mut self, name: &str) -> Result<(), String> {
        let template = self
            .templates
            .get(name)
            .ok_or_else(|| format!("Template '{}' not found", name))?;

        if template.builtin {
            return Err(format!("Cannot delete built-in template '{}'", name));
        }

        let path = self.template_path(name);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete template file: {}", e))?;
        }

        self.templates.remove(name);
        Ok(())
    }

    /// Export a template as a JSON string.
    pub fn export_template(&self, name: &str) -> Result<String, String> {
        let template = self
            .templates
            .get(name)
            .ok_or_else(|| format!("Template '{}' not found", name))?;

        serde_json::to_string_pretty(template).map_err(|e| format!("Failed to serialize: {}", e))
    }

    /// Import a template from a JSON string or URL.
    pub fn import_template_json(&mut self, json: &str) -> Result<Template, String> {
        let template: Template =
            serde_json::from_str(json).map_err(|e| format!("Invalid template JSON: {}", e))?;

        if template.name.is_empty() || template.trigger.is_empty() {
            return Err("Template must have a name and trigger".to_string());
        }

        if self.templates.contains_key(&template.name) {
            return Err(format!("Template '{}' already exists", template.name));
        }

        self.create_template(&template)?;
        Ok(template)
    }

    /// Import a template from a URL.
    pub async fn import_template_url(&mut self, url: &str) -> Result<Template, String> {
        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch URL: {}", e))?;

        let json = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        self.import_template_json(&json)
    }

    // ── Helpers ──

    fn template_path(&self, name: &str) -> PathBuf {
        self.templates_dir.join(format!("{}.json", name))
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the default templates directory: ~/.baoclaw/templates/
pub fn default_templates_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".baoclaw").join("templates")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use crate::engine::template::types::Variable;
    fn test_engine() -> (TemplateEngine, TempDir) {
        let dir = TempDir::new().unwrap();
        let engine = TemplateEngine::with_dir(dir.path().to_path_buf());
        (engine, dir)
    }

    #[test]
    fn test_builtin_templates_loaded() {
        let (engine, _dir) = test_engine();
        let review = engine.find_by_name("Code Review");
        assert!(review.is_some());
        assert_eq!(review.unwrap().trigger, "/review");

        let bugfix = engine.find_by_name("Bug Fix");
        assert!(bugfix.is_some());
        assert_eq!(bugfix.unwrap().trigger, "/bugfix");

        // All 5 builtins should be present
        let all = engine.list_all();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_match_trigger() {
        let (engine, _dir) = test_engine();

        let matched = engine.match_trigger("/review");
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().name, "Code Review");

        let matched2 = engine.match_trigger("/review --target main");
        assert!(matched2.is_some());
        assert_eq!(matched2.unwrap().name, "Code Review");

        let no_match = engine.match_trigger("/unknown");
        assert!(no_match.is_none());
    }

    #[test]
    fn test_list_all() {
        let (engine, _dir) = test_engine();
        let all = engine.list_all();
        assert_eq!(all.len(), 5);
        // Should be sorted by name
        assert_eq!(all[0].name, "Bug Fix");
        assert_eq!(all[1].name, "Code Review");
        assert_eq!(all[2].name, "Documentation");
        assert_eq!(all[3].name, "Feature Implementation");
        assert_eq!(all[4].name, "Refactoring");
    }

    #[test]
    fn test_search() {
        let (engine, _dir) = test_engine();

        // Unique query: only "Code Review" carries the "security" tag.
        let results = engine.search("security");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Code Review");

        let results2 = engine.search("review");
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].name, "Code Review");

        // Broad query matches several builtins (name/description/tags);
        // just verify the expected template is among them.
        assert!(engine
            .search("code")
            .iter()
            .any(|t| t.name == "Code Review"));
    }

    #[test]
    fn test_collect_variables_with_defaults() {
        let (engine, _dir) = test_engine();
        let template = engine.find_by_name("Code Review").unwrap();

        let result = engine.collect_variables(template, HashMap::new());
        match result {
            VariableCollectResult::Resolved(vars) => {
                assert_eq!(vars.get("target_branch").unwrap(), "main");
                assert_eq!(vars.get("include_tests").unwrap(), "true");
            }
            _ => panic!("Expected Resolved"),
        }
    }

    #[test]
    fn test_create_and_delete_template() {
        let (mut engine, _dir) = test_engine();

        let tpl = Template {
            name: "My Custom".into(),
            trigger: "/custom".into(),
            description: "A custom template".into(),
            ..Template::new("My Custom", "/custom")
        };

        engine.create_template(&tpl).unwrap();
        assert!(engine.find_by_name("My Custom").is_some());
        assert_eq!(engine.list_all().len(), 6);

        engine.delete_template("My Custom").unwrap();
        assert!(engine.find_by_name("My Custom").is_none());
        assert_eq!(engine.list_all().len(), 5);
    }

    #[test]
    fn test_cannot_delete_builtin() {
        let (mut engine, _dir) = test_engine();
        let err = engine.delete_template("Code Review").unwrap_err();
        assert!(err.contains("Cannot delete built-in"));
    }

    #[test]
    fn test_cannot_modify_builtin() {
        let (mut engine, _dir) = test_engine();
        let tpl = Template::new("Modified", "/review");
        let err = engine.update_template("Code Review", &tpl).unwrap_err();
        assert!(err.contains("Cannot modify built-in"));
    }

    #[test]
    fn test_export_import() {
        let (mut engine, _dir) = test_engine();

        let original = Template {
            name: "ExportTest".into(),
            trigger: "/export".into(),
            description: "Test export/import".into(),
            system_prompt_addon: "Be helpful".into(),
            workflow: vec![WorkflowStep {
                step: "Step 1".into(),
                action: WorkflowAction::Bash {
                    command: "echo hello".into(),
                    capture_output: true,
                },
                condition: None,
            }],
            variables: {
                let mut m = HashMap::new();
                m.insert(
                    "name".to_string(),
                    Variable {
                        default: "world".into(),
                        prompt: "Name?".into(),
                        required: true,
                        pattern: None,
                        help: String::new(),
                    },
                );
                m
            },
            ..Template::new("ExportTest", "/export")
        };

        engine.create_template(&original).unwrap();

        // Export
        let json = engine.export_template("ExportTest").unwrap();
        assert!(json.contains("ExportTest"));
        assert!(json.contains("export"));

        // Import shouldn't allow duplicate
        let err = engine.import_template_json(&json).unwrap_err();
        assert!(err.contains("already exists"));

        // Delete original, then re-import
        engine.delete_template("ExportTest").unwrap();
        let imported = engine.import_template_json(&json).unwrap();
        assert_eq!(imported.name, "ExportTest");
        assert_eq!(imported.trigger, "/export");
    }

    #[test]
    fn test_build_system_prompt() {
        let (engine, _dir) = test_engine();
        let template = engine.find_by_name("Code Review").unwrap();
        let mut vars = HashMap::new();
        vars.insert("target_branch".to_string(), "develop".to_string());

        let prompt = engine.build_system_prompt(template, &vars);
        assert!(prompt.contains("senior code reviewer"));
    }

    #[test]
    fn test_expand_steps_substitution() {
        let (engine, _dir) = test_engine();
        let template = engine.find_by_name("Code Review").unwrap();

        let mut vars = HashMap::new();
        vars.insert("target_branch".to_string(), "develop".to_string());
        vars.insert("include_tests".to_string(), "true".to_string());

        let steps = engine.expand_steps(template, &vars);
        assert!(!steps.is_empty());

        // First step should be bash with substituted branch
        let first = &steps[0];
        match &first.action {
            WorkflowAction::Bash { command, .. } => {
                assert!(command.contains("develop"));
                assert!(!command.contains("${target_branch}"));
            }
            _ => panic!("Expected Bash action"),
        }
    }

    #[test]
    fn test_expand_steps_condition_filtering() {
        let (engine, _dir) = test_engine();

        // Create a template with a conditional step
        let tpl = Template {
            name: "Conditional".into(),
            trigger: "/cond".into(),
            workflow: vec![
                WorkflowStep {
                    step: "Always runs".into(),
                    action: WorkflowAction::Bash {
                        command: "echo hello".into(),
                        capture_output: false,
                    },
                    condition: None,
                },
                WorkflowStep {
                    step: "Conditional".into(),
                    action: WorkflowAction::Bash {
                        command: "echo world".into(),
                        capture_output: false,
                    },
                    condition: Some("${flag}".into()),
                },
            ],
            variables: {
                let mut m = HashMap::new();
                m.insert(
                    "flag".to_string(),
                    Variable {
                        default: "true".into(),
                        prompt: String::new(),
                        required: false,
                        pattern: None,
                        help: String::new(),
                    },
                );
                m
            },
            ..Template::new("Conditional", "/cond")
        };

        // With flag=true
        let mut vars = HashMap::new();
        vars.insert("flag".to_string(), "true".to_string());
        let steps = engine.expand_steps(&tpl, &vars);
        assert_eq!(steps.len(), 2);

        // With flag=false
        let mut vars2 = HashMap::new();
        vars2.insert("flag".to_string(), "false".to_string());
        let steps2 = engine.expand_steps(&tpl, &vars2);
        assert_eq!(steps2.len(), 1);
    }
}
