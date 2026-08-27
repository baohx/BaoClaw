//! Template engine — workflow-based session templates.
//!
//! Provides predefined workflows triggered by slash commands:
//! - `/review` — Code Review workflow
//! - `/bugfix` — Bug Fix workflow
//! - `/feature` — Feature Implementation workflow
//! - `/docs` — Documentation generation
//! - `/refactor` — Safe refactoring workflow

pub mod builtins;
pub mod engine;
pub mod types;

#[cfg(test)]
mod engine_tests;
