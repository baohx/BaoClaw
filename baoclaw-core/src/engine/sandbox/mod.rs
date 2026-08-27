//! Enhanced Sandbox execution environment.
//!
//! Provides fine-grained control over sandbox isolation with:
//! - Network whitelisting (domain/port restrictions)
//! - Environment variable filtering
//! - Memory/CPU limits
//! - Multiple backend support (Bubblewrap, Docker, None)
//! - Permission escalation flow (detect, confirm, temp/permanent)
//! - Audit logging (SQLite-backed, all security decisions)

mod audit;
mod config;
mod executor;
#[path = "../sandbox_legacy.rs"]
mod legacy;
mod network;
mod permission;
mod profile;

// Public exports

// Re-export legacy types for backward compatibility
pub use legacy::{SandboxBackend, SandboxConfig};
