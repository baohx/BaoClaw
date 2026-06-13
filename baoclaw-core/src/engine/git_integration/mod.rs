//! Git integration module — PR management, branch operations,
//! conflict resolution, commit operations, and platform authentication.
//!
//! # Sub-modules
//! - `types` — Shared data types (PrInfo, BranchInfo, ConflictInfo, CommitInfo)
//! - `pr` — Pull request management via GitHub CLI (`gh`)
//! - `branch` — Branch create/switch/sync/cleanup/list
//! - `conflict` — Merge/rebase conflict detection and resolution
//! - `commit` — Squash, amend, undo, blame, history
//! - `auth` — GitHub/GitLab token management

pub mod types;
pub mod pr;
pub mod branch;
pub mod conflict;
pub mod commit;
pub mod auth;

// Re-export all public types for convenient access
