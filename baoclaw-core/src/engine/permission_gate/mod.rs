//! Engine-level permission gate — thin wrapper around the canonical `permissions` module.
//!
//! ## Architecture
//!
//! The **canonical** permission types live in [`crate::permissions`] (top-level):
//! - `permissions::gate::PermissionGate` — runtime request/response channel for tool
//!   execution permission (used by `ToolExecutor` to await CLI user decisions).
//! - `permissions::gate::PermissionDecision` — the user's allow/deny/allow-always decision.
//! - `permissions::manager::PermissionManager` — rule-based policy engine
//!   (`PermissionMode`, `PermissionRule`, `PermissionResult`).
//!
//! This module (`engine::permission_gate`) provides **engine-specific** extensions:
//! - [`gate::RuleBasedPermissionGate`] — a rule-evaluation engine with built-in safety
//!   defaults and session-aware caching (distinct from the canonical channel-based gate).
//! - [`types`] — engine-internal data types: `EnginePermissionDecision`, `DecisionType`,
//!   `PermissionRequest`, `CacheEntry`.
//! - [`cache`] — thread-safe permission cache for session/persistent grants.
//! - [`interactive`] — `InteractivePrompter`: format prompts, parse user responses.
//!
//! ## Re-exports
//!
//! Canonical types are re-exported here so that engine code can import everything
//! from a single path, eliminating ambiguity about which `PermissionGate` or
//! `PermissionDecision` to use:
//!
//! ```
//! use baoclaw_core::engine::permission_gate::{
//!     PermissionGate,          // canonical channel-based gate
//!     PermissionDecision,      // canonical user decision enum
//!     PermissionManager,       // canonical policy engine
//!     RuleBasedPermissionGate, // engine-specific rule evaluator
//!     DecisionType,            // engine-specific decision type
//! };
//! ```
//!
//! ## Deprecation note
//!
//! Direct use of `engine::permission_gate::gate::PermissionGate` is **deprecated**.
//! That struct has been renamed to [`gate::RuleBasedPermissionGate`] to disambiguate
//! from the canonical [`crate::permissions::gate::PermissionGate`].
//! Similarly, `engine::permission_gate::types::PermissionDecision` has been renamed
//! to [`types::EnginePermissionDecision`].

pub mod types;
pub mod cache;
pub mod gate;
pub mod interactive;

// ── Re-export canonical types from top-level `permissions` module ──────────
// These are the single source of truth for permission primitives.
// Callers should import these from here or from `crate::permissions` directly.
pub use crate::permissions::gate::{PermissionDecision, PermissionGate};
pub use crate::permissions::manager::{
    PermissionManager, PermissionMode, PermissionResult, PermissionRule, ToolPermissionContext,
};

// ── Re-export engine-specific types for convenient single-path access ──────
pub use gate::RuleBasedPermissionGate;
pub use types::{CacheEntry, DecisionType, EnginePermissionDecision, PermissionRequest};
