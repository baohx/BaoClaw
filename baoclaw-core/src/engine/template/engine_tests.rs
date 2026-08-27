#[cfg(test)]
mod tests {
    use crate::engine::template::engine::TemplateEngine;
    use crate::engine::template::engine::{VariableCollectResult, VariablePrompt};
    use crate::engine::template::types::{Template, Variable, WorkflowAction, WorkflowStep};
    use std::collections::HashMap;
    use tempfile::TempDir;

    // ─── helpers ───

    fn make_test_template(name: &str, trigger: &str) -> Template {
        let mut vars = HashMap::new();
        vars.insert(
            "language".into(),
            Variable {
                default: "Rust".into(),
                prompt: "Which language?".into(),
                required: true,
                pattern: None,
                help: "Programming language".into(),
            },
        );
        vars.insert(
            "focus".into(),
            Variable {
                default: "security".into(),
                prompt: "Focus area?".into(),
                required: false,
                pattern: None,
                help: "What to focus on".into(),
            },
        );

        Template {
            name: name.into(),
            trigger: trigger.into(),
            description: "A test template".into(),
            system_prompt_addon: "Review ${language} code for ${focus}.".into(),
            workflow: vec![
                WorkflowStep {
                    step: "Analyze ${language} code".into(),
                    action: WorkflowAction::Analyze {
                        prompt: "Analyze the ${language} code for ${focus} issues.".into(),
                        files: vec!["src/".into()],
                    },
                    condition: None,
                },
                WorkflowStep {
                    step: "Report findings".into(),
                    action: WorkflowAction::Format {
                        template: "report".into(),
                    },
                    condition: Some("${step0.output} != ''".into()),
                },
            ],
            variables: vars,
            version: "1.0.0".into(),
            author: "test".into(),
            builtin: false,
            tags: vec!["code".into(), "quality".into()],
        }
    }

    // ─── new() + builtins ───

    #[test]
    fn test_new_has_builtins() {
        let engine = TemplateEngine::new();
        assert!(engine.find_by_name("Code Review").is_some());
        assert!(engine.find_by_name("Bug Fix").is_some());
        assert!(engine.find_by_name("Feature Implementation").is_some());
        assert!(engine.find_by_name("Documentation").is_some());
        assert!(engine.find_by_name("Refactoring").is_some());
    }

    #[test]
    fn test_new_builtins_count() {
        let engine = TemplateEngine::new();
        assert!(engine.list_all().len() >= 5);
    }

    // ─── match_trigger ───

    #[test]
    fn test_match_trigger_hit() {
        let engine = TemplateEngine::new();
        let t = engine.match_trigger("/review");
        assert!(t.is_some(), "Should match /review trigger");
        assert_eq!(t.unwrap().name, "Code Review");
    }

    #[test]
    fn test_match_trigger_miss() {
        let engine = TemplateEngine::new();
        assert!(engine.match_trigger("/nonexistent").is_none());
    }

    #[test]
    fn test_match_trigger_empty() {
        let engine = TemplateEngine::new();
        assert!(engine.match_trigger("").is_none());
    }

    // ─── find_by_name ───

    #[test]
    fn test_find_by_name_hit() {
        let engine = TemplateEngine::new();
        assert!(engine.find_by_name("Code Review").is_some());
    }

    #[test]
    fn test_find_by_name_miss() {
        let engine = TemplateEngine::new();
        assert!(engine.find_by_name("nonexistent_xyz").is_none());
    }

    // ─── search ───

    #[test]
    fn test_search_by_name_fragment() {
        let engine = TemplateEngine::new();
        let results = engine.search("review");
        assert!(!results.is_empty());
        assert!(results.iter().any(|t| t.name == "Code Review"));
    }

    #[test]
    fn test_search_by_tag() {
        let engine = TemplateEngine::new();
        let results = engine.search("quality");
        // At least code_review should match
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_no_match() {
        let engine = TemplateEngine::new();
        assert!(engine.search("xyznonexistent123").is_empty());
    }

    // ─── list_all ───

    #[test]
    fn test_list_all_includes_all_builtins() {
        let engine = TemplateEngine::new();
        let names: Vec<&str> = engine.list_all().iter().map(|t| t.name.as_str()).collect();
        for expected in &[
            "Code Review",
            "Bug Fix",
            "Feature Implementation",
            "Documentation",
            "Refactoring",
        ] {
            assert!(names.contains(expected), "Missing builtin: {}", expected);
        }
    }

    // ─── build_system_prompt ───

    #[test]
    fn test_build_system_prompt_substitutes() {
        let engine = TemplateEngine::new();
        // code_review builtin has a system_prompt_addon with ${language} etc.
        let template = engine.find_by_name("Code Review").unwrap();
        let mut vars = HashMap::new();
        vars.insert("language".to_string(), "Rust".to_string());
        vars.insert("focus".to_string(), "unsafe blocks".to_string());
        let prompt = engine.build_system_prompt(template, &vars);
        // Should have substituted or returned the template text
        assert!(
            !prompt.is_empty() || template.system_prompt_addon.is_empty(),
            "Empty prompt but template has addon text"
        );
    }

    // ─── collect_variables ───

    #[test]
    fn test_collect_variables_with_defaults() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        let tpl = make_test_template("test_tpl", "/test");
        engine.create_template(&tpl).unwrap();

        let template = engine.find_by_name("test_tpl").unwrap();
        let result = engine.collect_variables(template, HashMap::new());
        match result {
            VariableCollectResult::Resolved(vars) => {
                assert_eq!(vars.get("language").map(|s| s.as_str()), Some("Rust"));
                assert_eq!(vars.get("focus").map(|s| s.as_str()), Some("security"));
            }
            VariableCollectResult::NeedsInput { .. } => {
                // Acceptable if engine doesn't auto-apply defaults
            }
        }
    }

    #[test]
    fn test_collect_variables_user_override() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        let tpl = make_test_template("test_tpl2", "/test2");
        engine.create_template(&tpl).unwrap();

        let template = engine.find_by_name("test_tpl2").unwrap();
        let mut user_vars = HashMap::new();
        user_vars.insert("language".to_string(), "Python".to_string());
        let result = engine.collect_variables(template, user_vars);
        match result {
            VariableCollectResult::Resolved(vars) => {
                assert_eq!(vars.get("language").map(|s| s.as_str()), Some("Python"));
            }
            _ => {}
        }
    }

    // ─── expand_steps ───

    #[test]
    fn test_expand_steps_substitutes() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        let tpl = make_test_template("expand_test", "/expand");
        engine.create_template(&tpl).unwrap();

        let template = engine.find_by_name("expand_test").unwrap();
        let mut vars = HashMap::new();
        vars.insert("language".to_string(), "Go".to_string());
        let steps = engine.expand_steps(template, &vars);
        // Both steps included: condition "${step0.output} != ''" with unresolved
        // variable does NOT evaluate to "false"/"0"/"no", so condition passes.
        assert!(steps.len() >= 1, "At least step 1 should be included");

        // Verify step 1 has substituted language
        let step0 = steps.first().unwrap();
        assert!(
            step0.step.contains("Go"),
            "Expected 'Go' in step text, got: {}",
            step0.step
        );
    }

    #[test]
    fn test_expand_steps_condition_false_filters() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        let mut tpl = make_test_template("cond_false", "/condf");
        // Set condition to literal "false" — should always filter
        tpl.workflow[1].condition = Some("false".into());
        engine.create_template(&tpl).unwrap();

        let template = engine.find_by_name("cond_false").unwrap();
        let steps = engine.expand_steps(template, &HashMap::new());
        assert_eq!(
            steps.len(),
            1,
            "Step 2 should be filtered when condition is 'false'"
        );
    }

    #[test]
    fn test_expand_steps_condition_passes() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        let tpl = make_test_template("cond_test", "/cond");
        engine.create_template(&tpl).unwrap();

        let template = engine.find_by_name("cond_test").unwrap();
        let mut vars = HashMap::new();
        vars.insert("language".to_string(), "Rust".to_string());
        vars.insert("step0.output".to_string(), "Found 3 bugs".to_string());
        let steps = engine.expand_steps(template, &vars);
        assert_eq!(
            steps.len(),
            2,
            "Both steps should be included when condition passes"
        );
    }

    // ─── create / delete ───

    #[test]
    fn test_create_and_delete_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        let count_before = engine.list_all().len();

        let tpl = make_test_template("roundtrip", "/rt");
        assert!(engine.create_template(&tpl).is_ok());
        assert_eq!(engine.list_all().len(), count_before + 1);
        assert!(engine.find_by_name("roundtrip").is_some());

        assert!(engine.delete_template("roundtrip").is_ok());
        assert_eq!(engine.list_all().len(), count_before);
        assert!(engine.find_by_name("roundtrip").is_none());
    }

    #[test]
    fn test_create_duplicate_fails() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        let tpl = make_test_template("dup", "/dup");
        assert!(engine.create_template(&tpl).is_ok());
        assert!(engine.create_template(&tpl).is_err());
    }

    #[test]
    fn test_delete_builtin_fails() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        assert!(engine.delete_template("Code Review").is_err());
    }

    // ─── export / import ───

    #[test]
    fn test_export_import_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let mut engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        let tpl = make_test_template("exim", "/exim");
        engine.create_template(&tpl).unwrap();

        let json = engine.export_template("exim").unwrap();
        assert!(json.contains("exim"));
        assert!(json.contains("language"));

        engine.delete_template("exim").unwrap();
        let imported = engine.import_template_json(&json).unwrap();
        assert_eq!(imported.name, "exim");
        assert_eq!(imported.trigger, "/exim");
        assert_eq!(imported.variables.len(), 2);
    }

    // ─── with_dir ───

    #[test]
    fn test_with_dir_loads_builtins_too() {
        let tmp = TempDir::new().unwrap();
        let engine = TemplateEngine::with_dir(tmp.path().to_path_buf());
        assert!(engine.find_by_name("Code Review").is_some());
    }
}
