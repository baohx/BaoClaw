//! Enhanced Sandbox execution environment.
//!
//! Provides fine-grained control over sandbox isolation with:
//! - Network whitelisting (domain/port restrictions)
//! - Environment variable filtering
//! - Memory/CPU limits
//! - Multiple backend support (Bubblewrap, Docker, None)
//! - Permission escalation flow (detect, confirm, temp/permanent)
//! - Audit logging (SQLite-backed, all security decisions)

mod profile;
mod executor;
mod network;
mod config;
mod audit;
mod permission;
#[path = "../sandbox_legacy.rs"]
mod legacy;

// Public exports
pub use profile::{SandboxProfile, NetworkRule, ProfilePreset};
pub use executor::SandboxExecutor;
pub use network::{NetworkWhitelist, HostMatcher, WhitelistRule};
pub use config::{SandboxConfigFile, load_sandbox_config, sandbox_config_path};
pub use audit::{AuditLog, AuditEvent, AuditEventRecord, audit_db_path};
pub use permission::{PermissionManager, EscalationRequest, EscalationResult};

// Re-export legacy types for backward compatibility
pub use legacy::{SandboxBackend, SandboxConfig};
