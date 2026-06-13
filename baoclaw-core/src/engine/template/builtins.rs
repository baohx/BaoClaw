//! Built-in workflow templates embedded in the binary.
//!
//! Five templates are provided out-of-the-box:
//! - Code Review (`/review`)
//! - Bug Fix (`/bugfix`)
//! - Feature Implementation (`/feature`)
//! - Documentation (`/docs`)
//! - Refactoring (`/refactor`)

use super::types::{Template, Variable, WorkflowAction, WorkflowStep};

/// All built-in templates (lazy-initialized via functions because
/// Rust const evaluation cannot allocate Vec/HashMap).
pub fn builtin_templates() -> Vec<Template> {
    vec![
        code_review(),
        bug_fix(),
        feature(),
        docs(),
        refactor(),
    ]
}

fn code_review() -> Template {
    Template {
        name: "Code Review".into(),
        trigger: "/review".into(),
        description: "Comprehensive code review with security, performance, and maintainability analysis".into(),
        system_prompt_addon: "You are a senior code reviewer. Focus on: security vulnerabilities, performance bottlenecks, \n\
             maintainability issues, code style consistency, and test coverage. \n\
             For each issue found, explain the risk and suggest a concrete fix. \n\
             Use the ${target_branch} branch as the comparison baseline.".into(),
        workflow: vec![
            WorkflowStep {
                step: "Get changed files compared to target branch".into(),
                action: WorkflowAction::Bash {
                    command: "git diff --name-only ${target_branch}".into(),
                    capture_output: true,
                },
                condition: None,
            },
            WorkflowStep {
                step: "Analyze each changed file for issues".into(),
                action: WorkflowAction::Analyze {
                    prompt: "Review the following changed files for security vulnerabilities, \n\
                            performance issues, maintainability problems, and style violations. \n\
                            For each issue: explain the risk, show the problematic code, and suggest a fix.".into(),
                    files: vec!["${step1.output}".into()],
                },
                condition: None,
            },
            WorkflowStep {
                step: "Generate structured review report".into(),
                action: WorkflowAction::Format {
                    template: "review-checklist".into(),
                },
                condition: None,
            },
        ],
        variables: {
            let mut m = std::collections::HashMap::new();
            m.insert("target_branch".into(), Variable {
                default: "main".into(),
                prompt: "Target branch for comparison?".into(),
                required: true,
                pattern: None,
                help: "The branch to compare against for code changes".into(),
            });
            m.insert("include_tests".into(), Variable {
                default: "true".into(),
                prompt: "Include test files in review?".into(),
                required: false,
                pattern: None,
                help: "Whether to include test file changes in the review scope".into(),
            });
            m
        },
        version: "1.0.0".into(),
        author: "BaoClaw".into(),
        builtin: true,
        tags: vec!["review".into(), "code-quality".into(), "security".into()],
    }
}

fn bug_fix() -> Template {
    Template {
        name: "Bug Fix".into(),
        trigger: "/bugfix".into(),
        description: "Systematic bug identification and resolution workflow".into(),
        system_prompt_addon: "You are a debugging expert. Follow a systematic approach: \n\
            1. Reproduce the bug first\n\
            2. Understand the root cause\n\
            3. Design a minimal fix\n\
            4. Verify the fix doesn't break anything\n\
            Always ask for reproduction steps if not provided.".into(),
        workflow: vec![
            WorkflowStep {
                step: "Describe the bug and reproduction steps".into(),
                action: WorkflowAction::Ask {
                    question: "Describe the bug and provide reproduction steps".into(),
                    variable: "bug_description".into(),
                    default: None,
                },
                condition: None,
            },
            WorkflowStep {
                step: "Find related files and code".into(),
                action: WorkflowAction::Analyze {
                    prompt: "Based on this bug description, identify the likely source files and code areas involved:\n\
                            ${bug_description}\n\n\
                            Search the codebase and list the files and functions most likely related to this bug.".into(),
                    files: Vec::new(),
                },
                condition: None,
            },
            WorkflowStep {
                step: "Identify root cause and propose fix".into(),
                action: WorkflowAction::Analyze {
                    prompt: "Analyze the identified code and determine the root cause of the bug. \n\
                            Propose a minimal fix that addresses the root cause without introducing side effects. \n\
                            Include: root cause analysis, proposed code change, and verification steps.".into(),
                    files: vec!["${step2.output}".into()],
                },
                condition: None,
            },
        ],
        variables: {
            let mut m = std::collections::HashMap::new();
            m.insert("bug_priority".into(), Variable {
                default: "medium".into(),
                prompt: "Bug priority (low/medium/high/critical)?".into(),
                required: false,
                pattern: Some(r"^(low|medium|high|critical)$".into()),
                help: "Used to determine urgency and resource allocation".into(),
            });
            m
        },
        version: "1.0.0".into(),
        author: "BaoClaw".into(),
        builtin: true,
        tags: vec!["debug".into(), "bugfix".into(), "troubleshooting".into()],
    }
}

fn feature() -> Template {
    Template {
        name: "Feature Implementation".into(),
        trigger: "/feature".into(),
        description: "End-to-end feature implementation workflow from spec to code".into(),
        system_prompt_addon: "You are a full-stack developer implementing a new feature. \n\
            Follow the specification carefully and implement incrementally. \n\
            Write tests before implementation code (TDD). \n\
            Keep changes minimal and focused on the feature requirements.".into(),
        workflow: vec![
            WorkflowStep {
                step: "Define feature specification".into(),
                action: WorkflowAction::Ask {
                    question: "Describe the feature to implement (what, why, acceptance criteria)".into(),
                    variable: "feature_spec".into(),
                    default: None,
                },
                condition: None,
            },
            WorkflowStep {
                step: "Break down into tasks".into(),
                action: WorkflowAction::Analyze {
                    prompt: "Based on this feature specification, break it down into concrete implementation tasks:\n\
                            ${feature_spec}\n\n\
                            Create a task list with: task name, description, estimated complexity, and dependencies.".into(),
                    files: Vec::new(),
                },
                condition: None,
            },
            WorkflowStep {
                step: "Implement the feature".into(),
                action: WorkflowAction::Analyze {
                    prompt: "Implement the feature according to the task breakdown. \n\
                            Follow TDD: write tests first, then implement the code. \n\
                            Feature spec: ${feature_spec}\n\
                            Task breakdown: ${step2.output}\n\n\
                            Proceed with implementation, starting with the first task.".into(),
                    files: Vec::new(),
                },
                condition: None,
            },
        ],
        variables: {
            let mut m = std::collections::HashMap::new();
            m.insert("use_tdd".into(), Variable {
                default: "true".into(),
                prompt: "Use Test-Driven Development?".into(),
                required: false,
                pattern: None,
                help: "If true, tests will be written before implementation code".into(),
            });
            m.insert("target_file".into(), Variable {
                default: String::new(),
                prompt: "Target file or directory for changes?".into(),
                required: false,
                pattern: None,
                help: "Specify where the feature should be implemented (leave empty to auto-detect)".into(),
            });
            m
        },
        version: "1.0.0".into(),
        author: "BaoClaw".into(),
        builtin: true,
        tags: vec!["feature".into(), "implementation".into(), "tdd".into()],
    }
}

fn docs() -> Template {
    Template {
        name: "Documentation".into(),
        trigger: "/docs".into(),
        description: "Generate or improve documentation for code".into(),
        system_prompt_addon: "You are a technical writer specializing in developer documentation. \n\
            Write clear, concise documentation that helps developers understand and use the code. \n\
            Include: overview, API reference, usage examples, and common patterns. \n\
            Follow the project's existing documentation style.".into(),
        workflow: vec![
            WorkflowStep {
                step: "Identify documentation target".into(),
                action: WorkflowAction::Select {
                    question: "What type of documentation do you need?".into(),
                    variable: "doc_type".into(),
                    options: vec![
                        "API Reference".into(),
                        "README".into(),
                        "Architecture Overview".into(),
                        "Code Comments".into(),
                        "Migration Guide".into(),
                    ],
                    default: "API Reference".into(),
                },
                condition: None,
            },
            WorkflowStep {
                step: "Analyze code and generate documentation".into(),
                action: WorkflowAction::Analyze {
                    prompt: "Generate ${doc_type} documentation for the target code. \n\
                            Analyze the code structure, public APIs, and usage patterns. \n\
                            Produce documentation that is accurate, comprehensive, and follows best practices.".into(),
                    files: vec!["${target_path}".into()],
                },
                condition: None,
            },
        ],
        variables: {
            let mut m = std::collections::HashMap::new();
            m.insert("target_path".into(), Variable {
                default: ".".into(),
                prompt: "Target file or directory to document?".into(),
                required: true,
                pattern: None,
                help: "The file or directory to generate documentation for".into(),
            });
            m.insert("format".into(), Variable {
                default: "markdown".into(),
                prompt: "Output format (markdown/rst/docx)?".into(),
                required: false,
                pattern: Some(r"^(markdown|rst|docx)$".into()),
                help: "The format for the generated documentation".into(),
            });
            m
        },
        version: "1.0.0".into(),
        author: "BaoClaw".into(),
        builtin: true,
        tags: vec!["docs".into(), "documentation".into(), "writing".into()],
    }
}

fn refactor() -> Template {
    Template {
        name: "Refactoring".into(),
        trigger: "/refactor".into(),
        description: "Safe code refactoring with validation and testing".into(),
        system_prompt_addon: "You are a refactoring expert. Follow safe refactoring principles: \n\
            1. Make small, incremental changes\n\
            2. Run tests after each change\n\
            3. Preserve existing behavior (no functionality changes)\n\
            4. Improve readability while maintaining performance\n\
            5. Document significant structural changes".into(),
        workflow: vec![
            WorkflowStep {
                step: "Specify refactoring goals".into(),
                action: WorkflowAction::Ask {
                    question: "What do you want to refactor and why?".into(),
                    variable: "refactor_goal".into(),
                    default: None,
                },
                condition: None,
            },
            WorkflowStep {
                step: "Analyze current code and plan refactoring".into(),
                action: WorkflowAction::Analyze {
                    prompt: "Analyze the target code for refactoring opportunities: \n\
                            ${refactor_goal}\n\n\
                            Identify code smells, propose improvements, and create a step-by-step refactoring plan. \n\
                            Each step should be small and verifiable (run tests after each step).".into(),
                    files: vec!["${target_path}".into()],
                },
                condition: None,
            },
            WorkflowStep {
                step: "Execute refactoring".into(),
                action: WorkflowAction::Analyze {
                    prompt: "Execute the refactoring plan step by step. After each step, run the test suite. \n\
                            Refactoring plan: ${step2.output}\n\n\
                            Proceed with the first step of the refactoring.".into(),
                    files: Vec::new(),
                },
                condition: None,
            },
        ],
        variables: {
            let mut m = std::collections::HashMap::new();
            m.insert("target_path".into(), Variable {
                default: ".".into(),
                prompt: "Target file or directory to refactor?".into(),
                required: true,
                pattern: None,
                help: "The code to refactor".into(),
            });
            m.insert("run_tests".into(), Variable {
                default: "true".into(),
                prompt: "Run tests after each change?".into(),
                required: false,
                pattern: None,
                help: "If true, the test suite will be run after each refactoring step".into(),
            });
            m
        },
        version: "1.0.0".into(),
        author: "BaoClaw".into(),
        builtin: true,
        tags: vec!["refactor".into(), "clean-code".into(), "maintenance".into()],
    }
}
