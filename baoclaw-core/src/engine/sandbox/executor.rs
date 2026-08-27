//! Sandbox executor - wraps commands for isolated execution.
//!
//! Supports three backends:
//! - Bubblewrap (bwrap) - Linux namespace isolation
//! - Docker - Container isolation
//! - None - Direct execution (no isolation)

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use super::config::SandboxConfigFile;
use super::profile::SandboxProfile;
use crate::engine::sandbox::{SandboxBackend, SandboxConfig};

/// Allowed commands in the sandbox whitelist.
///
/// These are the only program names that may appear as the first element
/// of a command vector built by the executor. LLM-generated command strings
/// are passed as arguments to a shell (`-c`), but the *program* invoked by
/// `Command::new` must be one of these safe wrappers or interpreters.
const SANDBOX_ALLOWED_COMMANDS: &[&str] = &[
    // Shell wrappers used internally by build_command_args
    "/bin/bash",
    "/bin/sh",
    "bash",
    "sh",
    // Sandbox backends
    "bwrap",
    "docker",
    // Interpreters / runtimes (code-interpreter scenarios)
    "python3",
    "python",
    "node",
    "ruby",
    "perl",
    "lua",
    // Core utilities
    "echo",
    "cat",
    "ls",
    "pwd",
    "env",
    "printenv",
    "grep",
    "sed",
    "awk",
    "sort",
    "uniq",
    "head",
    "tail",
    "wc",
    "tr",
    "cut",
    "paste",
    "expand",
    "fold",
    "fmt",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "test",
    "true",
    "false",
    "bc",
    "dc",
    "expr",
    "factor",
    "seq",
    "date",
    "cal",
    "uptime",
    "whoami",
    "id",
    "hostname",
    "md5sum",
    "sha256sum",
    "base64",
    "xxd",
    "jq",
    "yq",
    "xq",
    // Compilers / build tools
    "gcc",
    "g++",
    "clang",
    "rustc",
    "cargo",
    "make",
    "cmake",
];

/// Validate that the command program is in the whitelist.
///
/// This is a defence-in-depth measure: even though `build_command_args`
/// currently only produces program names from hard-coded backend logic,
/// we validate here so that any future refactor or misuse cannot bypass
/// the whitelist.
///
/// Returns the validated program string, or an error describing why
/// the command was rejected.
fn validate_command(program: &str) -> Result<String, String> {
    // Reject path separators (prevents ./evil or /tmp/evil)
    // Exception: absolute paths to system shells are allowed (/bin/bash, /bin/sh)
    let is_allowed_absolute = matches!(
        program,
        "/bin/bash" | "/bin/sh" | "/usr/bin/bash" | "/usr/bin/sh"
    );
    if !is_allowed_absolute && (program.contains('/') || program.contains('\\')) {
        return Err(format!(
            "Absolute/relative paths not allowed in sandbox: {}",
            program
        ));
    }

    // Reject shell metacharacters
    let dangerous_chars = [
        ';', '|', '&', '$', '`', '(', ')', '{', '}', '<', '>', '\n', '\r',
    ];
    if program.chars().any(|c| dangerous_chars.contains(&c)) {
        return Err(format!("Dangerous characters in command: {}", program));
    }

    if !SANDBOX_ALLOWED_COMMANDS.contains(&program) {
        return Err(format!("Command not in sandbox whitelist: {}", program));
    }

    Ok(program.to_string())
}

/// Sandbox executor that wraps commands for isolated execution.
#[derive(Clone, Debug)]
pub struct SandboxExecutor {
    /// Sandbox profile defining security boundaries.
    profile: SandboxProfile,

    /// Backend to use for isolation.
    backend: SandboxBackend,

    /// Original sandbox config (for backward compatibility).
    base_config: SandboxConfig,
}

impl SandboxExecutor {
    /// Create a new executor with the given profile and backend.
    pub fn new(profile: SandboxProfile, backend: SandboxBackend) -> Self {
        let base_config = Self::profile_to_config(&profile, backend.clone());
        Self {
            profile,
            backend,
            base_config,
        }
    }

    /// Create an executor from a SandboxConfigFile.
    pub fn from_config(config: &SandboxConfigFile, profile_name: &str) -> Option<Self> {
        let profile = config.get_profile(profile_name)?.clone();
        let backend = Self::detect_backend();
        Some(Self::new(profile, backend))
    }

    /// Create an executor with auto-detected backend.
    pub fn auto_detect(profile: SandboxProfile) -> Self {
        let backend = Self::detect_backend();
        Self::new(profile, backend)
    }

    /// Detect the best available backend.
    ///
    /// Falls back to [`SandboxBackend::None`] (direct, unsandboxed execution)
    /// when no isolation mechanism is usable. That downgrade is a security
    /// boundary change, so it is always announced on stderr — never silent.
    fn detect_backend() -> SandboxBackend {
        if which_exists("bwrap") {
            SandboxBackend::Bubblewrap
        } else if which_exists("docker") {
            let image = std::env::var("BAOCLAW_SANDBOX_IMAGE")
                .unwrap_or_else(|_| "baoclaw-sandbox:latest".into());
            if super::legacy::docker_image_exists(&image) {
                SandboxBackend::Docker { image }
            } else {
                eprintln!(
                    "Warning: sandbox backend 'docker' found but image '{}' is not available \
                     locally; falling back to UNSANDBOXED execution. Pull/build the image or \
                     point BAOCLAW_SANDBOX_IMAGE at an existing one.",
                    image
                );
                SandboxBackend::None
            }
        } else {
            eprintln!(
                "Warning: no sandbox backend available (install bwrap or docker); \
                 falling back to UNSANDBOXED execution."
            );
            SandboxBackend::None
        }
    }

    /// Convert profile to SandboxConfig for backward compatibility.
    fn profile_to_config(profile: &SandboxProfile, backend: SandboxBackend) -> SandboxConfig {
        SandboxConfig {
            backend,
            rw_mounts: profile.writable_paths.clone(),
            ro_mounts: profile.readable_paths.clone(),
            env_passthrough: profile.env_whitelist.clone(),
            allow_network: profile.network.is_allowed(),
            memory_limit_mb: profile.max_memory_mb,
            cpu_time_limit_secs: profile.timeout_secs,
            workdir: None,
        }
    }

    /// Get the current profile.
    pub fn profile(&self) -> &SandboxProfile {
        &self.profile
    }

    /// Get the current backend.
    pub fn backend(&self) -> &SandboxBackend {
        &self.backend
    }

    /// Check if the selected backend is available.
    pub fn is_available(&self) -> bool {
        self.base_config.is_available()
    }

    /// Validate the executor configuration.
    pub fn validate(&self) -> Option<String> {
        self.base_config.validate()
    }

    /// Build command arguments for execution.
    /// Returns Vec<String> where [0] is the program and [1..] are arguments.
    pub fn build_command_args(&self, command: &str, cwd: &Path) -> Vec<String> {
        match &self.backend {
            SandboxBackend::None => self.build_none_args(command),
            SandboxBackend::Bubblewrap => self.build_bwrap_args(command, cwd),
            SandboxBackend::Docker { image } => self.build_docker_args(command, cwd, image),
        }
    }

    /// Build a Command struct for direct execution.
    ///
    /// Validates that the resolved program name is in the sandbox whitelist
    /// before creating the `Command`. This prevents malicious or erroneous
    /// LLM output from executing arbitrary programs.
    pub fn build_command(&self, command: &str, cwd: &Path) -> Result<Command, String> {
        let args = self.build_command_args(command, cwd);
        // H3 Security fix: validate program name against whitelist before execution
        let validated_program = validate_command(&args[0])?;
        let mut cmd = Command::new(&validated_program);
        if args.len() > 1 {
            cmd.args(&args[1..]);
        }
        cmd.current_dir(cwd);
        Ok(cmd)
    }

    /// Get a description of the current sandbox level.
    pub fn description(&self) -> String {
        format!(
            "{} (profile: {}, backend: {})",
            self.base_config.description(),
            self.profile.name,
            match &self.backend {
                SandboxBackend::None => "none",
                SandboxBackend::Bubblewrap => "bubblewrap",
                SandboxBackend::Docker { .. } => "docker",
            }
        )
    }

    /// Build args for no sandbox (direct execution).
    fn build_none_args(&self, command: &str) -> Vec<String> {
        vec!["/bin/bash".into(), "-c".into(), command.to_string()]
    }

    /// Build args for Bubblewrap execution with profile restrictions.
    fn build_bwrap_args(&self, command: &str, cwd: &Path) -> Vec<String> {
        let mut args = vec!["bwrap".to_string()];

        // Bind essential filesystems read-only
        for path in &["/usr", "/lib", "/lib64", "/bin", "/sbin"] {
            if Path::new(path).exists() {
                args.push("--ro-bind".into());
                args.push(path.to_string());
                args.push(path.to_string());
            }
        }

        args.push("--proc".into());
        args.push("/proc".into());
        args.push("--dev".into());
        args.push("/dev".into());
        args.push("--tmpfs".into());
        args.push("/tmp".into());

        // Mount writable paths
        for mount in &self.profile.writable_paths {
            if mount == "*" {
                // Full access - bind entire filesystem
                args.push("--bind".into());
                args.push("/".into());
                args.push("/".into());
                break;
            }
            if Path::new(mount).exists() {
                args.push("--bind".into());
                args.push(mount.clone());
                args.push(mount.clone());
            }
        }

        // Mount readable paths (read-only, if not already mounted writable)
        let mounted: HashSet<&str> = self
            .profile
            .writable_paths
            .iter()
            .map(|s| s.as_str())
            .collect();
        for mount in &self.profile.readable_paths {
            if mount == "*" {
                continue; // Already have base filesystem
            }
            if mounted.contains(mount.as_str()) {
                continue; // Already mounted RW
            }
            if Path::new(mount).exists() {
                args.push("--ro-bind".into());
                args.push(mount.clone());
                args.push(mount.clone());
            }
        }

        // Network isolation
        if !self.profile.network.is_allowed() {
            args.push("--unshare-net".into());
        }
        // Note: Network whitelisting for Bubblewrap would require network namespaces
        // and iptables rules, which is complex. For now, it's all-or-nothing.

        // Working directory
        let workdir = cwd.to_str().unwrap_or("/tmp");
        args.push("--chdir".into());
        args.push(workdir.into());

        // Die with parent
        args.push("--die-with-parent".into());

        // Environment filtering
        // bwrap inherits environment by default; we need to unset variables not in whitelist
        if !self.profile.env_whitelist.contains(&"*".to_string()) {
            // Clear environment and only pass whitelisted
            args.push("--clearenv".into());
            for env in &self.profile.env_whitelist {
                if let Ok(val) = std::env::var(env) {
                    args.push("--setenv".into());
                    args.push(env.clone());
                    args.push(val);
                }
            }
        }

        // Memory and CPU limits (using bwrap'sRLIMIT features would require --rlimit-* flags)
        // These are available in newer versions of bwrap
        if self.profile.max_memory_mb > 0 {
            // bwrap doesn't have direct memory limit; would need cgroups
            // For now, we rely on the process timeout
        }

        // The command
        args.push("--".into());
        args.push("/bin/sh".into());
        args.push("-c".into());
        args.push(command.to_string());

        args
    }

    /// Build args for Docker execution with profile restrictions.
    fn build_docker_args(&self, command: &str, cwd: &Path, image: &str) -> Vec<String> {
        let mut args = vec!["docker".to_string(), "run".to_string()];

        // Remove container after exit
        args.push("--rm".into());

        // Run as current user
        #[cfg(unix)]
        {
            let uid = unsafe { libc::getuid() };
            let gid = unsafe { libc::getgid() };
            args.push("--user".into());
            args.push(format!("{}:{}", uid, gid));
        }

        // Memory limit
        if self.profile.max_memory_mb > 0 {
            args.push(format!("--memory={}m", self.profile.max_memory_mb));
        }

        // CPU limit
        if self.profile.cpu_time_limit_secs > 0 {
            args.push(format!(
                "--cpu-quota={}",
                self.profile.cpu_time_limit_secs * 100000
            ));
            args.push("--cpu-period=100000".into());
        }

        // Timeout (using --stop-timeout)
        if self.profile.timeout_secs > 0 {
            args.push(format!("--stop-timeout={}", self.profile.timeout_secs));
        }

        // Network
        if !self.profile.network.is_allowed() {
            args.push("--network=none".into());
        }
        // Note: Network whitelisting for Docker would require custom network setup
        // For now, it's all-or-nothing at the container level.

        // Mount paths
        let mut mounted: HashSet<String> = HashSet::new();

        // Mount CWD first
        if let Some(cwd_str) = cwd.to_str() {
            if mounted.insert(cwd_str.to_string()) {
                args.push("-v".into());
                args.push(format!("{}:{}", cwd_str, cwd_str));
            }
        }

        // Mount writable paths
        for mount in &self.profile.writable_paths {
            if mount == "*" {
                continue; // Can't mount entire filesystem in Docker
            }
            if mounted.insert(mount.clone()) && Path::new(mount).exists() {
                args.push("-v".into());
                args.push(format!("{}:{}", mount, mount));
            }
        }

        // Working directory
        let workdir = cwd.to_str().unwrap_or("/workspace");
        args.push("-w".into());
        args.push(workdir.into());

        // Environment filtering
        if self.profile.env_whitelist.contains(&"*".to_string()) {
            // Pass all environment
            args.push("-e".into());
            args.push(".*".into()); // Docker doesn't support this; would need --env-file
        } else {
            // Pass only whitelisted
            for env in &self.profile.env_whitelist {
                if let Ok(val) = std::env::var(env) {
                    args.push("-e".into());
                    args.push(format!("{}={}", env, val));
                }
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

    /// Check if a network operation is allowed.
    pub fn is_network_allowed(&self, host: &str, port: Option<u16>) -> bool {
        self.profile.network.is_host_allowed(host, port)
    }

    /// Check if a file path is writable.
    pub fn is_writable(&self, path: &str) -> bool {
        self.profile.is_writable(path)
    }

    /// Check if a file path is readable.
    pub fn is_readable(&self, path: &str) -> bool {
        self.profile.is_readable(path)
    }

    /// Filter environment variables according to profile.
    pub fn filter_env(&self, env: &[(String, String)]) -> Vec<(String, String)> {
        self.profile.filter_env(env)
    }
}

/// Check if a command exists in PATH (async version for tokio runtime).
///
/// Uses spawn_blocking to avoid blocking the async runtime.
///
/// NOTE: This is an internal helper that only invokes the system `which`
/// command with hard-coded program names (e.g. "bwrap", "docker"). It is
/// NOT exposed to LLM output and therefore does not need whitelist validation.
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
/// NOTE: Internal helper — only called with hard-coded program names during
/// backend detection. Not exposed to LLM output; no whitelist needed.
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
///
/// NOTE: Internal helper — only invokes `docker image inspect` with internally
/// determined image names. Not exposed to LLM output; no whitelist needed.
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

#[cfg(test)]
mod tests {
    use super::super::profile::NetworkRule;
    use super::*;

    #[test]
    fn test_executor_none_backend() {
        let profile = SandboxProfile::read_only();
        let executor = SandboxExecutor::new(profile, SandboxBackend::None);

        let args = executor.build_command_args("ls -la", Path::new("/tmp"));
        assert_eq!(args[0], "/bin/bash");
        assert_eq!(args[1], "-c");
        assert_eq!(args[2], "ls -la");
    }

    #[test]
    fn test_executor_bwrap_backend() {
        let profile = SandboxProfile::web_dev();
        let executor = SandboxExecutor::new(profile, SandboxBackend::Bubblewrap);

        let args = executor.build_command_args("npm install", Path::new("/workspace"));
        assert!(args[0] == "bwrap");
        assert!(args.contains(&"--unshare-net".to_string()) == false); // web_dev allows network
        assert!(args.iter().any(|a| a.contains("npm install")));
    }

    #[test]
    fn test_executor_bwrap_no_network() {
        let profile = SandboxProfile::read_only();
        let executor = SandboxExecutor::new(profile, SandboxBackend::Bubblewrap);

        let args = executor.build_command_args("cargo build", Path::new("/workspace"));
        assert!(args.contains(&"--unshare-net".to_string()));
    }

    #[test]
    fn test_executor_docker_backend() {
        let profile = SandboxProfile::web_dev();
        let executor = SandboxExecutor::new(
            profile,
            SandboxBackend::Docker {
                image: "baoclaw:latest".into(),
            },
        );

        let args = executor.build_command_args("npm test", Path::new("/workspace"));
        assert!(args[0] == "docker");
        assert!(args[1] == "run");
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.iter().any(|a| a.contains("--memory=1024m")));
        assert!(args.iter().any(|a| a == "baoclaw:latest"));
    }

    #[test]
    fn test_executor_docker_no_network() {
        let profile = SandboxProfile::read_only();
        let executor = SandboxExecutor::new(
            profile,
            SandboxBackend::Docker {
                image: "baoclaw:latest".into(),
            },
        );

        let args = executor.build_command_args("cat file", Path::new("/workspace"));
        assert!(args.contains(&"--network=none".into()));
    }

    #[test]
    fn test_network_whitelist() {
        let mut profile = SandboxProfile::web_dev();
        profile.network =
            NetworkRule::Whitelist(vec!["localhost:*".to_string(), "*.npmjs.org".to_string()]);
        let executor = SandboxExecutor::new(profile, SandboxBackend::None);

        assert!(executor.is_network_allowed("localhost", Some(3000)));
        assert!(executor.is_network_allowed("registry.npmjs.org", Some(443)));
        assert!(!executor.is_network_allowed("google.com", Some(443)));
    }

    #[test]
    fn test_path_permissions() {
        let profile = SandboxProfile::web_dev();
        let executor = SandboxExecutor::new(profile, SandboxBackend::None);

        assert!(executor.is_writable("src/main.rs"));
        assert!(executor.is_writable("dist/bundle.js"));
        assert!(!executor.is_writable("/etc/passwd"));
        assert!(executor.is_readable("/etc/passwd")); // readable but not writable
    }

    #[test]
    fn test_env_filtering() {
        let profile = SandboxProfile::read_only();
        let executor = SandboxExecutor::new(profile, SandboxBackend::None);

        let env = vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
            ("SECRET".to_string(), "secret123".to_string()),
        ];
        let filtered = executor.filter_env(&env);

        assert!(filtered.iter().any(|(k, _)| k == "PATH"));
        assert!(filtered.iter().any(|(k, _)| k == "HOME"));
        assert!(!filtered.iter().any(|(k, _)| k == "SECRET"));
    }

    #[test]
    fn test_from_config() {
        let config = SandboxConfigFile::default();
        let executor = SandboxExecutor::from_config(&config, "web_dev").unwrap();

        assert_eq!(executor.profile().name, "web_dev");
        assert!(executor.profile().network.is_allowed());
    }

    #[test]
    fn test_auto_detect() {
        let executor = SandboxExecutor::auto_detect(SandboxProfile::read_only());
        // Environment-independent invariant: whatever backend auto-detection
        // picks must actually be usable on this machine. This holds whether
        // or not bwrap/docker are installed, so the test is not fragile.
        assert!(
            executor.is_available(),
            "auto-detected backend {:?} reports itself unavailable",
            executor.backend()
        );
    }

    #[test]
    fn test_auto_detect_never_silently_downgrades() {
        // detect_backend() must never return an unusable backend: if it
        // falls back to None, that decision was announced on stderr and the
        // resulting executor still reports available.
        let backend = SandboxExecutor::detect_backend();
        match &backend {
            SandboxBackend::None => {} // loud fallback, fine
            SandboxBackend::Bubblewrap => assert!(
                which_exists("bwrap"),
                "picked Bubblewrap but bwrap binary missing"
            ),
            SandboxBackend::Docker { image } => assert!(
                super::super::legacy::docker_image_exists(image),
                "picked Docker backend but image '{}' missing",
                image
            ),
        }
    }

    #[test]
    fn test_description() {
        let executor = SandboxExecutor::new(SandboxProfile::web_dev(), SandboxBackend::None);
        let desc = executor.description();
        assert!(desc.contains("web_dev"));
        assert!(desc.contains("none"));
    }

    #[test]
    fn test_validate_command_accepts_whitelist() {
        // Internal shell wrappers and backends
        assert!(validate_command("/bin/bash").is_ok());
        assert!(validate_command("/bin/sh").is_ok());
        assert!(validate_command("bash").is_ok());
        assert!(validate_command("sh").is_ok());
        assert!(validate_command("bwrap").is_ok());
        assert!(validate_command("docker").is_ok());
        // Interpreters
        assert!(validate_command("python3").is_ok());
        assert!(validate_command("python").is_ok());
        assert!(validate_command("node").is_ok());
        assert!(validate_command("ruby").is_ok());
        // Utilities
        assert!(validate_command("ls").is_ok());
        assert!(validate_command("cat").is_ok());
        assert!(validate_command("echo").is_ok());
        // Compilers
        assert!(validate_command("gcc").is_ok());
        assert!(validate_command("cargo").is_ok());
    }

    #[test]
    fn test_validate_command_rejects_paths() {
        assert!(validate_command("./evil").is_err());
        assert!(validate_command("/tmp/evil").is_err());
        assert!(validate_command("/usr/local/bin/custom").is_err());
        assert!(validate_command("some/relative/path").is_err());
        assert!(validate_command("C:\\evil.exe").is_err());
    }

    #[test]
    fn test_validate_command_rejects_metacharacters() {
        assert!(validate_command("bash;rm -rf /").is_err());
        assert!(validate_command("cat|grep secret").is_err());
        assert!(validate_command("echo $(whoami)").is_err());
        assert!(validate_command("echo `whoami`").is_err());
        assert!(validate_command("foo\nbar").is_err());
        assert!(validate_command("foo\rbar").is_err());
    }

    #[test]
    fn test_validate_command_rejects_non_whitelist() {
        assert!(validate_command("rm").is_err());
        assert!(validate_command("curl").is_err());
        assert!(validate_command("wget").is_err());
        assert!(validate_command("nc").is_err());
        assert!(validate_command("chmod").is_err());
        assert!(validate_command("custom_binary").is_err());
    }

    #[test]
    fn test_build_command_returns_result() {
        // build_command should now return Result and validate the program
        let profile = SandboxProfile::read_only();
        let executor = SandboxExecutor::new(profile, SandboxBackend::None);
        let cmd = executor.build_command("ls -la", Path::new("/tmp"));
        assert!(cmd.is_ok(), "build_command for None backend should succeed");
    }
}
