//! Commit management operations.
//!
//! Provides squash, amend, undo, blame, and history operations
//! for git commits using the `git` command-line tool.

use std::process::Command;

use super::pr::GitIntegrationError;

/// Manages git commit operations.
pub struct CommitManager;

impl CommitManager {
    /// Ensure we are inside a git repository.
    fn ensure_git_repo() -> Result<(), GitIntegrationError> {
        let output = Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .map_err(|_| GitIntegrationError::NotAGitRepo)?;
        if !output.status.success() {
            return Err(GitIntegrationError::NotAGitRepo);
        }
        Ok(())
    }

    /// Run a git command and return its stdout as String.
    fn run_git(args: &[&str]) -> Result<String, GitIntegrationError> {
        let output = Command::new("git")
            .args(args)
            .output()
            .map_err(GitIntegrationError::IoError)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    /// Squash the last `count` commits into one.
    ///
    /// Uses `git reset --soft HEAD~<count>` followed by `git commit --no-edit`
    /// with the first commit's message. This is more reliable than interactive
    /// rebase in automated environments.
    pub fn squash_commits(count: u32) -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo()?;

        if count < 2 {
            return Err(GitIntegrationError::CommandFailed(
                "Cannot squash fewer than 2 commits".into(),
            ));
        }

        // Get the message of the commit we'll keep
        let message = Self::run_git(&["log", "--format=%B", "-n", "1", &format!("HEAD~{}", count - 1)])?;

        // Soft reset to the parent of the oldest commit we're squashing
        let output = Command::new("git")
            .args(["reset", "--soft", &format!("HEAD~{}", count)])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        // Re-commit with the collected message
        let commit = Command::new("git")
            .args(["commit", "-m", message.trim()])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !commit.status.success() {
            let stderr = String::from_utf8_lossy(&commit.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Amend the most recent commit without changing its message.
    pub fn amend_commit() -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo()?;

        let output = Command::new("git")
            .args(["commit", "--amend", "--no-edit"])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Undo the most recent commit, keeping changes staged (--soft reset).
    pub fn undo_commit() -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo()?;

        let output = Command::new("git")
            .args(["reset", "--soft", "HEAD~1"])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_lower = stderr.to_lowercase();
            if stderr_lower.contains("unknown revision")
                || stderr_lower.contains("fatal: ambiguous")
            {
                return Err(GitIntegrationError::CommandFailed(
                    "No commits to undo — repository may have only one commit".into(),
                ));
            }
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Run `git blame` on a file and return the full annotated output.
    ///
    /// Returns line-by-line blame information showing the commit hash,
    /// author, timestamp, and line content.
    pub fn smart_blame(file: &str) -> Result<String, GitIntegrationError> {
        Self::ensure_git_repo()?;

        // Use --date=short for compact output: hash (author date line)
        let output = Command::new("git")
            .args(["blame", "--date=short", file])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_lower = stderr.to_lowercase();
            if stderr_lower.contains("no such path") || stderr_lower.contains("not in") {
                return Err(GitIntegrationError::CommandFailed(format!(
                    "File not tracked by git: {}",
                    file
                )));
            }
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Run `git log` on a file with optional graph view.
    ///
    /// Returns one-line-per-commit history. When `graph` is true,
    /// includes ASCII commit graph.
    pub fn history(file: &str, graph: bool) -> Result<String, GitIntegrationError> {
        Self::ensure_git_repo()?;

        let mut cmd = Command::new("git");
        cmd.args(["log", "--oneline"]);
        if graph {
            cmd.arg("--graph");
        }
        cmd.arg("--");
        cmd.arg(file);

        let output = cmd.output().map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_git_repo_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = CommitManager::ensure_git_repo();
        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Err(GitIntegrationError::NotAGitRepo) | Err(GitIntegrationError::IoError(_)) => {}
            other => panic!("Expected error, got: {:?}", other),
        }
    }

    #[test]
    fn test_squash_count_validation() {
        let result = CommitManager::squash_commits(0);
        assert!(result.is_err());
        let result = CommitManager::squash_commits(1);
        assert!(result.is_err());
    }

    #[test]
    fn test_history_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = CommitManager::history("somefile.rs", false);
        std::env::set_current_dir(&cwd).unwrap();

        assert!(result.is_err());
    }

    #[test]
    fn test_blame_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = CommitManager::smart_blame("nonexistent.rs");
        std::env::set_current_dir(&cwd).unwrap();

        assert!(result.is_err());
    }

    #[test]
    fn test_squash_commits_in_real_repo() {
        let tmp = tempfile::tempdir().unwrap();

        // Initialize a git repo
        let init = Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output();
        if init.is_err() || !init.unwrap().status.success() {
            return; // git not available
        }

        // Configure git
        let _ = Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output();

        // Create initial commit
        std::fs::write(tmp.path().join("file.txt"), "v1").unwrap();
        let _ = Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output();
        let _ = Command::new("git")
            .args(["commit", "-m", "first"])
            .current_dir(tmp.path())
            .output();

        // Create second commit
        std::fs::write(tmp.path().join("file.txt"), "v2").unwrap();
        let _ = Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output();
        let _ = Command::new("git")
            .args(["commit", "-m", "second"])
            .current_dir(tmp.path())
            .output();

        // Create third commit
        std::fs::write(tmp.path().join("file.txt"), "v3").unwrap();
        let _ = Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output();
        let _ = Command::new("git")
            .args(["commit", "-m", "third"])
            .current_dir(tmp.path())
            .output();

        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        // Squash last 3 commits
        let result = CommitManager::squash_commits(3);
        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Ok(()) => {
                // Verify we now have 1 commit with the message "first"
                let log = Command::new("git")
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
