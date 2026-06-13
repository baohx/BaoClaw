//! Interactive Permission Gate module.
//!
//! Provides fine-grained, interactive permission control for tool access.
//! The permission gate evaluates tool actions against configurable rules,
//! caches user grants, and supports interactive confirmation prompts.
//!
//! ## Architecture
//!
//! ```text
//! Tool Request → PermissionGate.check()
//!     ├── Check cache (PermissionCache)
//!     │   └── Hit: return cached Decision
//!     └── Evaluate rules (Vec<PermissionRule>)
//!         ├── Match → Decision (Allow/Deny/AskUser)
//!         └── No match → AskUser (default)
//! ```
//!
//! ## Modules
//!
//! - [`types`] — Core data types: rules, requests, decisions, cache entries
//! - [`gate`] — PermissionGate engine: rule evaluation, grant management
//! - [`cache`] — Thread-safe permission cache for session/persistent grants
//! - [`interactive`] — InteractivePrompter: format prompts, parse user responses

pub mod types;
pub mod cache;
pub mod gate;
pub mod interactive;

// Re-export all public types for convenient access
