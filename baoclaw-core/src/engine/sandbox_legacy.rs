//! Sandbox execution environment — isolates tool execution from the host system.
//!
//! The `SandboxBackend`/`SandboxConfig` types live in `infra::sandbox_config`;
//! the behavioral impls below form a split inherent impl on the same types.

use std::path::Path;

pub use crate::infra::sandbox_config::{SandboxBackend, SandboxConfig};

impl SandboxConfig {
    /// Create config that auto-detects the best available backend.
    pub fn auto_detect() -> Self {
        let backend = if which_exists("bwrap") {
            SandboxBackend::Bubblewrap
        } else if which_exists("docker") {
            let image = std::env::var("BAOCLAW_SANDBOX_IMAGE")
                .unwrap_or_else(|_| "baoclaw-sandbox:latest".into());
            if docker_image_exists(&image) {
                SandboxBackend::Docker { image }
            } else {
                SandboxBackend::None
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
    /// Returns the full command string to execute (for display/logging).
    pub fn wrap_command(&self, command: &str, cwd: &Path) -> String {
        self.build_command_args(command, cwd).join(" ")
    }

    /// Build the command as a proper argument vector for direct execution.
    /// Returns Vec<String> where [0] is the program and [1..] are arguments.
    /// This avoids shell-quoting issues that occur with string joining.
    pub fn build_command_args(&self, command: &str, cwd: &Path) -> Vec<String> {
        match &self.backend {
            SandboxBackend::None => vec!["/bin/bash".into(), "-c".into(), command.to_string()],
            SandboxBackend::Bubblewrap => self.build_bwrap_args(command, cwd),
            SandboxBackend::Docker { image } => self.build_docker_args(command, cwd, image),
        }
    }

    fn wrap_bwrap(&self, command: &str, cwd: &Path) -> String {
        self.build_bwrap_args(command, cwd).join(" ")
    }

    fn build_bwrap_args(&self, command: &str, cwd: &Path) -> Vec<String> {
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

        args
    }

    fn wrap_docker(&self, command: &str, cwd: &Path, image: &str) -> String {
        self.build_docker_args(command, cwd, image).join(" ")
    }

    fn build_docker_args(&self, command: &str, cwd: &Path, image: &str) -> Vec<String> {
        let mut args = vec!["docker".to_string(), "run".to_string()];

        // Remove container after exit
        args.push("--rm".into());

        // Run as current user to avoid permission issues with mounted volumes
        #[cfg(unix)]
        {
            let uid = unsafe { libc::getuid() };
            let gid = unsafe { libc::getgid() };
            args.push("--user".into());
            args.push(format!("{}:{}", uid, gid));
        }

        // Memory limit
        if self.memory_limit_mb > 0 {
            args.push(format!("--memory={}m", self.memory_limit_mb));
        }

        // CPU limit (only if non-default to avoid overly restrictive quotas)
        if self.cpu_time_limit_secs > 0 && self.cpu_time_limit_secs < 300 {
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

        // Collect all mount paths (deduplicated)
        let mut mounted: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Mount CWD
        if let Some(cwd_str) = cwd.to_str() {
            if mounted.insert(cwd_str.to_string()) {
                args.push("-v".into());
                args.push(format!("{}:{}", cwd_str, cwd_str));
            }
        }

        // Additional RW mounts (deduplicated against CWD)
        for mount in &self.rw_mounts {
            if mounted.insert(mount.clone()) {
                // Only mount if the path exists on host
                if Path::new(mount).exists() {
                    args.push("-v".into());
                    args.push(format!("{}:{}", mount, mount));
                }
            }
        }

        // Working directory
        let workdir = self
            .workdir
            .as_deref()
            .unwrap_or_else(|| cwd.to_str().unwrap_or("/workspace"));
        args.push("-w".into());
        args.push(workdir.into());

        // Environment passthrough (pass actual values from host)
        for env in &self.env_passthrough {
            if let Ok(val) = std::env::var(env) {
                args.push("-e".into());
                args.push(format!("{}={}", env, val));
            }
        }

        // Image
        args.push(image.into());

        // Command
        args.push("/bin/sh".into());
        args.push("-c".into());
        args.push(command.to_string());

        args
    }

    /// Check if the selected backend is actually available.
    pub fn is_available(&self) -> bool {
        match &self.backend {
            SandboxBackend::None => true,
            SandboxBackend::Bubblewrap => which_exists("bwrap"),
            SandboxBackend::Docker { image } => {
                which_exists("docker") && docker_image_exists(image)
            }
        }
    }

    /// Validate the sandbox configuration and return a human-readable error if invalid.
    /// Returns None if everything is OK.
    pub fn validate(&self) -> Option<String> {
        match &self.backend {
            SandboxBackend::None => None,
            SandboxBackend::Bubblewrap => {
                if !which_exists("bwrap") {
                    Some("bwrap not found. Install bubblewrap: apt install bubblewrap".into())
                } else {
                    None
                }
            }
            SandboxBackend::Docker { image } => {
                if !which_exists("docker") {
                    return Some("docker not found in PATH".into());
                }
                if !docker_image_exists(image) {
                    Some(format!(
                        "Docker image '{}' not found. Build it with: docker build -t {} -f Dockerfile.sandbox .",
                        image, image
                    ))
                } else {
                    None
                }
            }
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

/// Check if a command exists in PATH (async version for tokio runtime).
///
/// Uses spawn_blocking to avoid blocking the async runtime.
///
/// NOTE: This is a legacy internal helper. Only invoked with hard-coded
/// program names (e.g. "bwrap", "docker") during backend detection.
/// Not exposed to LLM output; no whitelist validation needed.
pub async fn which_exists_async(cmd: &str) -> bool {
    let cmd = cmd.to_string();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new("which")
            .arg(&cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// Check if a command exists in PATH (synchronous, for non-async contexts).
///
/// NOTE: Legacy internal helper — only called with hard-coded program names.
/// Not exposed to LLM output; no whitelist validation needed.
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if a Docker image exists locally (async version).
pub async fn docker_image_exists_async(image: &str) -> bool {
    let image = image.to_string();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new("docker")
            .args(["image", "inspect", &image])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// Check if a Docker image exists locally (synchronous, for non-async contexts).
pub(crate) fn docker_image_exists(image: &str) -> bool {
    std::process::Command::new("docker")
        .args(["image", "inspect", image])
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
        // Default backend is None → build_command_args returns ["/bin/bash", "-c", "ls -la"]
        let args = config.build_command_args("ls -la", Path::new("/tmp"));
        assert_eq!(args, vec!["/bin/bash", "-c", "ls -la"]);
        // wrap_command joins them for display
        assert_eq!(config.wrap_command("ls -la", Path::new("/tmp")), "/bin/bash -c ls -la");
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
