//! Sandbox execution environment — isolates tool execution from the host system.

use serde::{Deserialize, Serialize};
use std::path::Path;

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

impl SandboxConfig {
    /// Create config that auto-detects the best available backend.
    pub fn auto_detect() -> Self {
        let backend = if which_exists("bwrap") {
            SandboxBackend::Bubblewrap
        } else if which_exists("docker") {
            SandboxBackend::Docker {
                image: "baoclaw-sandbox:latest".into(),
            }
        } else {
            SandboxBackend::None
        };
        Self {
            backend,
            ..Self::default()
        }
    }

    /// Wrap a command for sandboxed execution.
    /// Returns the full command string to execute.
    pub fn wrap_command(&self, command: &str, cwd: &Path) -> String {
        match &self.backend {
            SandboxBackend::None => command.to_string(),
            SandboxBackend::Bubblewrap => self.wrap_bwrap(command, cwd),
            SandboxBackend::Docker { image } => self.wrap_docker(command, cwd, image),
        }
    }

    fn wrap_bwrap(&self, command: &str, cwd: &Path) -> String {
        let mut args = vec!["bwrap".to_string()];

        // Bind host filesystem read-only by default
        args.push("--ro-bind".into());
        args.push("/usr".into());
        args.push("/usr".into());

        args.push("--ro-bind".into());
        args.push("/lib".into());
        args.push("/lib".into());

        args.push("--ro-bind".into());
        args.push("/lib64".into());
        args.push("/lib64".into());

        args.push("--ro-bind".into());
        args.push("/bin".into());
        args.push("/bin".into());

        args.push("--ro-bind".into());
        args.push("/sbin".into());
        args.push("/sbin".into());

        args.push("--proc".into());
        args.push("/proc".into());

        args.push("--dev".into());
        args.push("/dev".into());

        args.push("--tmpfs".into());
        args.push("/tmp".into());

        // RW mounts
        for mount in &self.rw_mounts {
            if Path::new(mount).exists() {
                args.push("--bind".into());
                args.push(mount.clone());
                args.push(mount.clone());
            }
        }

        // RO mounts
        for mount in &self.ro_mounts {
            if Path::new(mount).exists() {
                args.push("--ro-bind".into());
                args.push(mount.clone());
                args.push(mount.clone());
            }
        }

        // Network
        if !self.allow_network {
            args.push("--unshare-net".into());
        }

        // Working directory
        let workdir = self
            .workdir
            .as_deref()
            .unwrap_or_else(|| cwd.to_str().unwrap_or("/tmp"));
        args.push("--chdir".into());
        args.push(workdir.into());

        // Die with parent
        args.push("--die-with-parent".into());

        // The actual command
        args.push("--".into());
        args.push("/bin/sh".into());
        args.push("-c".into());
        args.push(command.to_string());

        args.join(" ")
    }

    fn wrap_docker(&self, command: &str, cwd: &Path, image: &str) -> String {
        let mut args = vec!["docker".to_string(), "run".to_string()];

        // Remove container after exit
        args.push("--rm".into());

        // Memory limit
        if self.memory_limit_mb > 0 {
            args.push(format!("--memory={}m", self.memory_limit_mb));
        }

        // CPU limit
        if self.cpu_time_limit_secs > 0 {
            // Docker doesn't have direct CPU time, use period+quota approximation
            args.push(format!(
                "--cpu-quota={}",
                self.cpu_time_limit_secs * 100000
            ));
            args.push("--cpu-period=100000".into());
        }

        // Network
        if !self.allow_network {
            args.push("--network=none".into());
        }

        // Mount CWD
        if let Some(cwd_str) = cwd.to_str() {
            args.push("-v".into());
            args.push(format!("{}:{}", cwd_str, cwd_str));
        }

        // Additional RW mounts
        for mount in &self.rw_mounts {
            args.push("-v".into());
            args.push(format!("{}:{}", mount, mount));
        }

        // Working directory
        let workdir = self
            .workdir
            .as_deref()
            .unwrap_or_else(|| cwd.to_str().unwrap_or("/workspace"));
        args.push("-w".into());
        args.push(workdir.into());

        // Environment
        for env in &self.env_passthrough {
            args.push("-e".into());
            args.push(env.clone());
        }

        // Image
        args.push(image.into());

        // Command
        args.push("/bin/sh".into());
        args.push("-c".into());
        args.push(command.to_string());

        args.join(" ")
    }

    /// Check if the selected backend is actually available.
    pub fn is_available(&self) -> bool {
        match &self.backend {
            SandboxBackend::None => true,
            SandboxBackend::Bubblewrap => which_exists("bwrap"),
            SandboxBackend::Docker { .. } => which_exists("docker"),
        }
    }

    /// Get a description of the current sandbox level.
    pub fn description(&self) -> &str {
        match &self.backend {
            SandboxBackend::None => "No sandbox (direct execution)",
            SandboxBackend::Bubblewrap => "Bubblewrap (Linux namespace isolation)",
            SandboxBackend::Docker { .. } => "Docker (container isolation)",
        }
    }
}

/// Check if a command exists in PATH.
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_sandbox_passthrough() {
        let config = SandboxConfig::default();
        assert_eq!(config.wrap_command("ls -la", Path::new("/tmp")), "ls -la");
    }

    #[test]
    fn test_auto_detect_returns_valid() {
        let config = SandboxConfig::auto_detect();
        assert!(config.is_available());
    }

    #[test]
    fn test_description() {
        let config = SandboxConfig::default();
        assert!(config.description().contains("No sandbox"));
    }

    #[test]
    fn test_bwrap_command_builds() {
        let config = SandboxConfig {
            backend: SandboxBackend::Bubblewrap,
            rw_mounts: vec!["/home/user/project".into()],
            ro_mounts: vec![],
            allow_network: true,
            workdir: Some("/home/user/project".into()),
            ..SandboxConfig::default()
        };
        let cmd = config.wrap_command("cargo test", Path::new("/home/user/project"));
        assert!(cmd.starts_with("bwrap"));
        assert!(cmd.contains("cargo test"));
        assert!(cmd.contains("/home/user/project"));
    }

    #[test]
    fn test_docker_command_builds() {
        let config = SandboxConfig {
            backend: SandboxBackend::Docker {
                image: "baoclaw:latest".into(),
            },
            rw_mounts: vec![],
            ro_mounts: vec![],
            allow_network: false,
            workdir: Some("/workspace".into()),
            ..SandboxConfig::default()
        };
        let cmd = config.wrap_command("cargo build", Path::new("/workspace"));
        assert!(cmd.starts_with("docker run"));
        assert!(cmd.contains("--network=none"));
        assert!(cmd.contains("baoclaw:latest"));
    }
}
