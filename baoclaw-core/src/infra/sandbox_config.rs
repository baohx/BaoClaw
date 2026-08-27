//! Sandbox configuration types.
//!
//! Moved here from `engine/sandbox_legacy.rs` so `tools` can reference
//! sandbox settings without depending on the `engine` domain. The
//! behavioral impls (command building, backend detection) remain in
//! `engine::sandbox_legacy` as a split inherent impl.

use serde::{Deserialize, Serialize};

/// Sandbox backend type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SandboxBackend {
    /// No sandbox — direct execution (for trusted environments).
    None,
    /// Bubblewrap (bwrap) — lightweight Linux namespace sandbox.
    Bubblewrap,
    /// Docker container isolation.
    Docker { image: String },
}

/// Configuration for sandbox execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Which backend to use.
    pub backend: SandboxBackend,
    /// Directories to mount read-write into the sandbox.
    pub rw_mounts: Vec<String>,
    /// Directories to mount read-only into the sandbox.
    pub ro_mounts: Vec<String>,
    /// Environment variables to pass through.
    pub env_passthrough: Vec<String>,
    /// Network access allowed.
    pub allow_network: bool,
    /// Memory limit in MB (0 = unlimited).
    pub memory_limit_mb: u32,
    /// CPU time limit in seconds (0 = unlimited).
    pub cpu_time_limit_secs: u32,
    /// Working directory inside sandbox.
    pub workdir: Option<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            backend: SandboxBackend::None,
            rw_mounts: Vec::new(),
            ro_mounts: Vec::new(),
            env_passthrough: vec![
                "HOME".into(),
                "PATH".into(),
                "TERM".into(),
                "http_proxy".into(),
                "https_proxy".into(),
            ],
            allow_network: true,
            memory_limit_mb: 0,
            cpu_time_limit_secs: 300, // 5 minute default
            workdir: None,
        }
    }
}
