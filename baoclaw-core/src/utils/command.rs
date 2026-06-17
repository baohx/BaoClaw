//! Async command execution utilities.
//!
//! Wraps `std::process::Command` in `tokio::task::spawn_blocking` to prevent
//! blocking the tokio async runtime's worker threads.
//!
//! Audit issue H1: all `std::process::Command` calls that run external processes
//! (git, gh, docker, which) must be wrapped in `spawn_blocking` when called from
//! an async context.

use std::path::Path;

/// Output of a command execution.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Exit status of the process.
    pub status: std::process::ExitStatus,
    /// Captured stdout as a UTF-8 string.
    pub stdout: String,
    /// Captured stderr as a UTF-8 string.
    pub stderr: String,
}

impl CommandOutput {
    /// Whether the command exited successfully.
    pub fn success(&self) -> bool {
        self.status.success()
    }
}

/// Run a command asynchronously using `spawn_blocking`.
///
/// This offloads the blocking `Command::output()` call to a dedicated thread pool,
/// preventing it from stalling the tokio async runtime.
///
/// # Arguments
/// * `program` - The executable to run (e.g. `"git"`, `"gh"`)
/// * `args` - Arguments to pass to the program
/// * `cwd` - Optional working directory
///
/// # Example
/// ```ignore
/// let output = run_command_async("git", &["status", "--porcelain"], Some(Path::new("."))).await?;
/// if output.success() {
///     println!("{}", output.stdout);
/// }
/// ```
pub async fn run_command_async(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
) -> std::io::Result<CommandOutput> {
    let program = program.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let cwd = cwd.map(|p| p.to_path_buf());

    tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args);
        if let Some(ref cwd) = cwd {
            cmd.current_dir(cwd);
        }
        let output = cmd.output()?;
        Ok(CommandOutput {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    })
    .await?
}

/// Run a command asynchronously with additional environment variables.
///
/// Like [`run_command_async`] but allows passing environment variables.
pub async fn run_command_async_with_env(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    env: &[(&str, &str)],
) -> std::io::Result<CommandOutput> {
    let program = program.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let cwd = cwd.map(|p| p.to_path_buf());
    let env: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args);
        if let Some(ref cwd) = cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &env {
            cmd.env(k, v);
        }
        let output = cmd.output()?;
        Ok(CommandOutput {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    })
    .await?
}

/// Run a command asynchronously and check if it succeeds (boolean only).
///
/// Convenience wrapper for existence checks like `which <cmd>`.
pub async fn command_exists_async(cmd: &str) -> bool {
    run_command_async("which", &[cmd], None)
        .await
        .map(|o| o.success())
        .unwrap_or(false)
}
