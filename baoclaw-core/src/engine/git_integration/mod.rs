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

pub mod auth;
pub mod branch;
pub mod commit;
pub mod conflict;
pub mod pr;
pub mod types;

// Re-export all public types for convenient access

// Shared test-only lock: tests that mutate the process-global working
// directory (`std::env::set_current_dir`) must serialize against each other,
// otherwise parallel test threads race and observe the wrong cwd.
#[cfg(test)]
pub(crate) static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
