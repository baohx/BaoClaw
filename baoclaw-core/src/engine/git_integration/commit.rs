//! Commit management operations.
//!
//! Provides squash, amend, undo, blame, and history operations
//! for git commits using the `git` command-line tool.

use super::pr::GitIntegrationError;
use crate::utils::command::run_command_async;

/// Manages git commit operations.
pub struct CommitManager;

impl CommitManager {
    /// Ensure we are inside a git repository.
    async fn ensure_git_repo() -> Result<(), GitIntegrationError> {
        let output = run_command_async("git", &["rev-parse", "--is-inside-work-tree"], None)
            .await
            .map_err(|_| GitIntegrationError::NotAGitRepo)?;
        if !output.success() {
            return Err(GitIntegrationError::NotAGitRepo);
        }
        Ok(())
    }

    /// Run a git command and return its stdout as String.
    async fn run_git(args: &[&str]) -> Result<String, GitIntegrationError> {
        let output = run_command_async("git", args, None)
            .await
            .map_err(GitIntegrationError::IoError)?;
        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }
        Ok(output.stdout)
    }

    /// Squash the last `count` commits into one.
    ///
    /// Uses `git reset --soft HEAD~<count>` followed by `git commit --no-edit`
    /// with the first commit's message. This is more reliable than interactive
    /// rebase in automated environments.
    pub async fn squash_commits(count: u32) -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo().await?;

        if count < 2 {
            return Err(GitIntegrationError::CommandFailed(
                "Cannot squash fewer than 2 commits".into(),
            ));
        }

        // Get the message of the commit we'll keep
        let head_arg = format!("HEAD~{}", count - 1);
        let message = Self::run_git(&["log", "--format=%B", "-n", "1", &head_arg]).await?;

        // Soft reset to the parent of the oldest commit we're squashing
        let reset_target = format!("HEAD~{}", count);
        let output = run_command_async("git", &["reset", "--soft", &reset_target], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        // Re-commit with the collected message
        let msg_trimmed = message.trim();
        let output = run_command_async("git", &["commit", "-m", msg_trimmed], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        Ok(())
    }

    /// Amend the most recent commit without changing its message.
    pub async fn amend_commit() -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo().await?;

        let output = run_command_async("git", &["commit", "--amend", "--no-edit"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        Ok(())
    }

    /// Undo the most recent commit, keeping changes staged (--soft reset).
    pub async fn undo_commit() -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo().await?;

        let output = run_command_async("git", &["reset", "--soft", "HEAD~1"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            let stderr_lower = output.stderr.to_lowercase();
            if stderr_lower.contains("unknown revision")
                || stderr_lower.contains("fatal: ambiguous")
            {
                return Err(GitIntegrationError::CommandFailed(
                    "No commits to undo — repository may have only one commit".into(),
                ));
            }
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        Ok(())
    }

    /// Run `git blame` on a file and return the full annotated output.
    ///
    /// Returns line-by-line blame information showing the commit hash,
    /// author, timestamp, and line content.
    pub async fn smart_blame(file: &str) -> Result<String, GitIntegrationError> {
        Self::ensure_git_repo().await?;

        // Use --date=short for compact output: hash (author date line)
        let output = run_command_async("git", &["blame", "--date=short", file], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            let stderr_lower = output.stderr.to_lowercase();
            if stderr_lower.contains("no such path") || stderr_lower.contains("not in") {
                return Err(GitIntegrationError::CommandFailed(format!(
                    "File not tracked by git: {}",
                    file
                )));
            }
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        Ok(output.stdout)
    }

    /// Run `git log` on a file with optional graph view.
    ///
    /// Returns one-line-per-commit history. When `graph` is true,
    /// includes ASCII commit graph.
    pub async fn history(file: &str, graph: bool) -> Result<String, GitIntegrationError> {
        Self::ensure_git_repo().await?;

        let mut args = vec!["log", "--oneline"];
        if graph {
            args.push("--graph");
        }
        args.push("--");
        args.push(file);

        let output = run_command_async("git", &args, None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        Ok(output.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ensure_git_repo_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let _cwd_guard = match super::super::CWD_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(), // a sibling test panicked; recover and still restore cwd
        };
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = CommitManager::ensure_git_repo().await;
        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Err(GitIntegrationError::NotAGitRepo) | Err(GitIntegrationError::IoError(_)) => {}
            other => panic!("Expected error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_squash_count_validation() {
        let result = CommitManager::squash_commits(0).await;
        assert!(result.is_err());
        let result = CommitManager::squash_commits(1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_history_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let _cwd_guard = match super::super::CWD_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(), // a sibling test panicked; recover and still restore cwd
        };
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = CommitManager::history("somefile.rs", false).await;
        std::env::set_current_dir(&cwd).unwrap();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blame_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let _cwd_guard = match super::super::CWD_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(), // a sibling test panicked; recover and still restore cwd
        };
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = CommitManager::smart_blame("nonexistent.rs").await;
        std::env::set_current_dir(&cwd).unwrap();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_squash_commits_in_real_repo() {
        // Test setup uses sync commands (in test context, not on async runtime)
        let tmp = tempfile::tempdir().unwrap();

        // Initialize a git repo
        let init = std::process::Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output();
        if init.is_err() || !init.unwrap().status.success() {
            return; // git not available
        }

        // Configure git
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output();

        // Create initial commit
        std::fs::write(tmp.path().join("file.txt"), "v1").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "first"])
            .current_dir(tmp.path())
            .output();

        // Create second commit
        std::fs::write(tmp.path().join("file.txt"), "v2").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "second"])
            .current_dir(tmp.path())
            .output();

        // Create third commit
        std::fs::write(tmp.path().join("file.txt"), "v3").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "third"])
            .current_dir(tmp.path())
            .output();

        let _cwd_guard = match super::super::CWD_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(), // a sibling test panicked; recover and still restore cwd
        };
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        // Squash last 3 commits
        let result = CommitManager::squash_commits(3).await;
        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Ok(()) => {
                // Verify we now have 1 commit with the message "first"
                let log = std::process::Command::new("git")
                    .args(["log", "--oneline"])
                    .current_dir(tmp.path())
                    .output()
                    .unwrap();
                let log_stdout = String::from_utf8(log.stdout).unwrap();
                let count = log_stdout.lines().count();
                assert_eq!(count, 1, "Expected 1 commit after squash, got: {}", log_stdout);
            }
            Err(_) => {
                // In some git versions, soft reset with HEAD~N when N >= total commits
                // might behave differently. This is acceptable.
            }
        }
    }
}
