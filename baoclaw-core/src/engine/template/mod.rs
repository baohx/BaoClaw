//! Template engine — workflow-based session templates.
//!
//! Provides predefined workflows triggered by slash commands:
//! - `/review` — Code Review workflow
//! - `/bugfix` — Bug Fix workflow
//! - `/feature` — Feature Implementation workflow
//! - `/docs` — Documentation generation
//! - `/refactor` — Safe refactoring workflow

pub mod types;
pub mod builtins;
pub mod engine;

pub use types::{Template, WorkflowStep, WorkflowAction, Variable};
pub use engine::{TemplateEngine, VariableCollectResult, VariablePrompt};
pub use builtins::builtin_templates;
