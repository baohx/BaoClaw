//! Template type definitions — JSON schema for workflow templates.
//!
//! Templates are stored as JSON files in `~/.baoclaw/templates/`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A complete workflow template.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Template {
    /// Human-readable name.
    pub name: String,
    /// The slash command that triggers this template (e.g. "/review").
    pub trigger: String,
    /// Short description shown in the template list.
    #[serde(default)]
    pub description: String,
    /// Additional text appended to the system prompt when this template is active.
    #[serde(default)]
    pub system_prompt_addon: String,
    /// Ordered list of workflow steps.
    #[serde(default)]
    pub workflow: Vec<WorkflowStep>,
    /// User-settable variables with defaults and prompts.
    #[serde(default)]
    pub variables: HashMap<String, Variable>,
    /// Template version (for sharing/updates).
    #[serde(default = "default_version")]
    pub version: String,
    /// Author name (for sharing).
    #[serde(default)]
    pub author: String,
    /// Whether this is a built-in template (cannot be deleted, only customized).
    #[serde(default)]
    pub builtin: bool,
    /// Tags for categorization and search.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

/// A single step in a workflow.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStep {
    /// Human-readable description of this step.
    pub step: String,
    /// The action to take.
    pub action: WorkflowAction,
    /// Condition for executing this step (optional). If the expression evaluates
    /// to false, this step is skipped. Supports ${variable} substitution.
    #[serde(default)]
    pub condition: Option<String>,
}

/// Actions supported in template workflows.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "params")]
pub enum WorkflowAction {
    /// Run a bash command with variable substitution.
    #[serde(rename = "bash")]
    Bash {
        command: String,
        #[serde(default)]
        capture_output: bool,
    },

    /// Send a message to the LLM for analysis/response.
    #[serde(rename = "analyze")]
    Analyze {
        /// Prompt to send. Supports ${variable} and ${stepN.output} substitution.
        prompt: String,
        /// Optional list of files to include as context.
        #[serde(default)]
        files: Vec<String>,
    },

    /// Format output using a specific template.
    #[serde(rename = "format")]
    Format {
        /// Template name or inline template content.
        template: String,
    },

    /// Ask the user for input.
    #[serde(rename = "ask")] 
    Ask {
        /// Question to display.
        question: String,
        /// Variable name to store the answer in.
        variable: String,
        #[serde(default)]
        default: Option<String>,
    },

    /// Select from a list of options.
    #[serde(rename = "select")]
    Select {
        question: String,
        variable: String,
        options: Vec<String>,
        #[serde(default)]
        default: String,
    },

    /// Execute another template as a sub-workflow.
    #[serde(rename = "sub_template")]
    SubTemplate {
        /// Name or trigger of the sub-template.
        name: String,
        /// Variables to pass.
        #[serde(default)]
        variables: HashMap<String, String>,
    },
}

/// User-settable variable definition.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Variable {
    /// Default value.
    #[serde(default)]
    pub default: String,
    /// Prompt shown when collecting the value.
    #[serde(default)]
    pub prompt: String,
    /// Whether this variable is required.
    #[serde(default = "default_true")]
    pub required: bool,
    /// Validation pattern (regex).
    #[serde(default)]
    pub pattern: Option<String>,
    /// Help text shown after the prompt.
    #[serde(default)]
    pub help: String,
}

fn default_true() -> bool {
    true
}

impl Template {
    /// Create a minimal template with just name and trigger.
    pub fn new(name: &str, trigger: &str) -> Self {
        Self {
            name: name.to_string(),
            trigger: trigger.to_string(),
            description: String::new(),
            system_prompt_addon: String::new(),
            workflow: Vec::new(),
            variables: HashMap::new(),
            version: default_version(),
            author: String::new(),
            builtin: false,
            tags: Vec::new(),
        }
    }

    /// Check if the trigger matches the given command prefix.
    pub fn matches_trigger(&self, command: &str) -> bool {
        command.starts_with(&self.trigger)
    }

    /// Substitute ${variable} placeholders with actual values.
    pub fn substitute(&self, text: &str, vars: &HashMap<String, String>) -> String {
        let mut result = text.to_string();
        for (key, value) in vars {
            result = result.replace(&format!("${{{}}}", key), value);
        }
        // Handle step outputs: ${stepN.output} (N is 1-indexed)
        for (key, value) in vars {
            if let Some(step_key) = key.strip_prefix("step") {
                if let Some(output_key) = step_key.strip_suffix(".output") {
                    result = result.replace(&format!("${{step{}.output}}", output_key), value);
                    result = result.replace(&format!("${{{}}}", key), value);
                }
            }
        }
        result
    }

    /// Get unresolved variables (those without defaults in the vars map).
    pub fn unresolved_variables(&self, vars: &HashMap<String, String>) -> Vec<String> {
        self.variables
            .iter()
            .filter(|(key, _)| !vars.contains_key(*key))
            .filter(|(_, var)| var.required)
            .map(|(key, _)| key.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_trigger() {
        let t = Template::new("Code Review", "/review");
        assert!(t.matches_trigger("/review"));
        assert!(t.matches_trigger("/review --target main"));
        assert!(!t.matches_trigger("/bugfix"));
    }

    #[test]
    fn test_substitute_variables() {
        let t = Template::new("Test", "/test");
        let mut vars = HashMap::new();
        vars.insert("target_branch".to_string(), "main".to_string());

        let result = t.substitute("git diff --name-only ${target_branch}", &vars);
        assert_eq!(result, "git diff --name-only main");
    }

    #[test]
    fn test_substitute_step_output() {
        let t = Template::new("Test", "/test");
        let mut vars = HashMap::new();
        vars.insert("step1.output".to_string(), "file1.rs\nfile2.rs".to_string());

        let result = t.substitute("Analyze ${step1.output}", &vars);
        assert_eq!(result, "Analyze file1.rs\nfile2.rs");
    }

    #[test]
    fn test_unresolved_variables() {
        let t = Template {
            name: "Test".into(),
            trigger: "/test".into(),
            variables: {
                let mut m = HashMap::new();
                m.insert("branch".to_string(), Variable {
                    default: String::new(),
                    prompt: "Which branch?".into(),
                    required: true,
                    pattern: None,
                    help: String::new(),
                });
                m.insert("optional".to_string(), Variable {
                    default: "no".into(),
                    prompt: "Optional?".into(),
                    required: false,
                    pattern: None,
                    help: String::new(),
                });
                m
            },
            ..Template::new("Test", "/test")
        };

        let mut vars = HashMap::new();
        vars.insert("optional".to_string(), "yes".to_string());

        let unresolved = t.unresolved_variables(&vars);
        assert_eq!(unresolved, vec!["branch"]);
    }

    #[test]
    fn test_template_new() {
        let t = Template::new("My Template", "/mytpl");
        assert_eq!(t.name, "My Template");
        assert_eq!(t.trigger, "/mytpl");
        assert_eq!(t.version, "1.0.0");
        assert!(t.workflow.is_empty());
        assert!(!t.builtin);
    }

    #[test]
    fn test_serialize_roundtrip() {
        let mut t = Template::new("Code Review", "/review");
        t.description = "Review code changes".into();
        t.system_prompt_addon = "Be thorough.".into();
        t.workflow = vec![WorkflowStep {
            step: "Get changes".into(),
            action: WorkflowAction::Bash {
                command: "git diff ${branch}".into(),
                capture_output: true,
            },
            condition: None,
        }];
        t.variables.insert("branch".to_string(), Variable {
            default: "main".into(),
            prompt: "Branch?".into(),
            required: true,
            pattern: None,
            help: String::new(),
        });
        t.tags = vec!["review".into(), "git".into()];

        let json = serde_json::to_string_pretty(&t).unwrap();
        let parsed: Template = serde_json::from_str(&json).unwrap();
        assert_eq!(t, parsed);
    }
}
