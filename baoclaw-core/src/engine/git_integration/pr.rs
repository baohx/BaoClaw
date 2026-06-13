//! PR (Pull Request) management via GitHub CLI (`gh`).
//!
//! Provides create/list/review/merge operations for pull requests
//! using the `gh` command-line tool.

use std::process::Command;

use super::types::PrInfo;

/// Error type for git integration operations.
#[derive(Debug, thiserror::Error)]
pub enum GitIntegrationError {
    #[error("gh CLI not found — please install GitHub CLI: https://cli.github.com")]
    GhCliNotFound,
    #[error("not inside a git repository")]
    NotAGitRepo,
    #[error("authentication failed — run `gh auth login` first")]
    AuthenticationFailed,
    #[error("merge conflict detected — resolve conflicts before merging")]
    MergeConflict,
    #[error("PR not found: {0}")]
    PrNotFound(String),
    #[error("git command failed: {0}")]
    CommandFailed(String),
    #[error("JSON parse error: {0}")]
    JsonParseError(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
}

/// Manages GitHub pull requests using the `gh` CLI tool.
pub struct PrManager;

impl PrManager {
    /// Check whether `gh` CLI is available.
    pub fn is_gh_available() -> bool {
        Command::new("gh")
            .args(["--version"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Ensure the `gh` CLI is installed, returning a friendly error otherwise.
    fn ensure_gh() -> Result<(), GitIntegrationError> {
        if !Self::is_gh_available() {
            return Err(GitIntegrationError::GhCliNotFound);
        }
        Ok(())
    }

    /// Check that the current directory is inside a git repository.
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

    /// Create a new pull request.
    ///
    /// # Arguments
    /// * `title` - PR title
    /// * `body` - PR description (optional)
    /// * `base` - Target branch (optional, defaults to repo default)
    ///
    /// Returns the created PR info parsed from `gh pr create --json` output.
    pub fn create_pr(
        title: &str,
        body: Option<&str>,
        base: Option<&str>,
    ) -> Result<PrInfo, GitIntegrationError> {
        Self::ensure_gh()?;
        Self::ensure_git_repo()?;

        let mut cmd = Command::new("gh");
        cmd.args(["pr", "create", "--title", title, "--json", "number,title,body,state,baseRefName,headRefName,author{login},createdAt,url"]);

        if let Some(b) = body {
            cmd.arg("--body");
            cmd.arg(b);
        }
        if let Some(b) = base {
            cmd.arg("--base");
            cmd.arg(b);
        }

        let output = cmd.output().map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_lower = stderr.to_lowercase();

            if stderr_lower.contains("auth") || stderr_lower.contains("unauthorized") {
                return Err(GitIntegrationError::AuthenticationFailed);
            }
            if stderr_lower.contains("conflict") {
                return Err(GitIntegrationError::MergeConflict);
            }
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let pr: PrInfo = serde_json::from_str(stdout.trim())?;
        Ok(pr)
    }

    /// List pull requests with optional state filter.
    ///
    /// # Arguments
    /// * `state` - Filter by state: "open", "closed", "merged", or None for all
    pub fn list_prs(state: Option<&str>) -> Result<Vec<PrInfo>, GitIntegrationError> {
        Self::ensure_gh()?;
        Self::ensure_git_repo()?;

        let mut cmd = Command::new("gh");
        cmd.args([
            "pr",
            "list",
            "--json",
            "number,title,body,state,baseRefName,headRefName,author{login},createdAt,url",
        ]);

        if let Some(s) = state {
            cmd.args(["--state", s]);
        }

        // Use a generous limit for listing
        cmd.args(["--limit", "100"]);

        let output = cmd.output().map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let prs: Vec<PrInfo> = serde_json::from_str(stdout.trim())?;
        Ok(prs)
    }

    /// Review/view a specific pull request by number.
    pub fn review_pr(number: u32) -> Result<PrInfo, GitIntegrationError> {
        Self::ensure_gh()?;
        Self::ensure_git_repo()?;

        let number_str = number.to_string();

        let output = Command::new("gh")
            .args([
                "pr",
                "view",
                &number_str,
                "--json",
                "number,title,body,state,baseRefName,headRefName,author{login},createdAt,url",
            ])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_lower = stderr.to_lowercase();
            if stderr_lower.contains("not found") || stderr_lower.contains("no pull request") {
                return Err(GitIntegrationError::PrNotFound(number_str));
            }
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let pr: PrInfo = serde_json::from_str(stdout.trim())?;
        Ok(pr)
    }

    /// Merge a pull request.
    ///
    /// # Arguments
    /// * `number` - PR number to merge
    /// * `squash` - If true, squash all commits into one before merging
    pub fn merge_pr(number: u32, squash: bool) -> Result<(), GitIntegrationError> {
        Self::ensure_gh()?;
        Self::ensure_git_repo()?;

        let number_str = number.to_string();
        let mut cmd = Command::new("gh");
        cmd.args(["pr", "merge", &number_str, "--delete-branch"]);

        if squash {
            cmd.arg("--squash");
        }

        let output = cmd.output().map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_lower = stderr.to_lowercase();

            if stderr_lower.contains("auth") || stderr_lower.contains("unauthorized") {
                return Err(GitIntegrationError::AuthenticationFailed);
            }
            if stderr_lower.contains("conflict") {
                return Err(GitIntegrationError::MergeConflict);
            }
            if stderr_lower.contains("not found") {
                return Err(GitIntegrationError::PrNotFound(number_str));
            }
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gh_availability_check() {
        // This test just ensures the check doesn't panic
        let available = PrManager::is_gh_available();
        // gh may or may not be installed in test env — both outcomes are valid
        // We just verify it returns a boolean
        assert!(available == true || available == false);
    }

    #[test]
    fn test_ensure_git_repo_in_non_repo() {
        // Create a temp dir that is NOT a git repo
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = PrManager::ensure_git_repo();

        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Err(GitIntegrationError::NotAGitRepo) => {} // expected
            Err(GitIntegrationError::IoError(_)) => {}  // also acceptable (git not installed)
            other => panic!("Expected NotAGitRepo or IoError, got: {:?}", other),
        }
    }

    #[test]
    fn test_list_prs_without_gh_cli() {
        // When gh is not available, list_prs should return GhCliNotFound
        if !PrManager::is_gh_available() {
            let result = PrManager::list_prs(None);
            assert!(result.is_err());
            match result {
                Err(GitIntegrationError::GhCliNotFound) => {} // expected
                other => panic!("Expected GhCliNotFound, got: {:?}", other),
            }
        }
    }

    #[test]
    fn test_pr_info_deserialization_github() {
        // Test that we can deserialize gh CLI JSON output format
        // where author is a nested object like {"login": "dev1"}
        let json = r#"{
            "number": 42,
            "title": "Test PR",
            "body": "PR body",
            "state": "OPEN",
            "baseRefName": "main",
            "headRefName": "feature/test",
            "author": {"login": "dev1"},
            "createdAt": "2026-01-15T10:30:00Z",
            "url": "https://github.com/org/repo/pull/42"
        }"#;

        let pr: PrInfo = serde_json::from_str(json).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "Test PR");
        assert_eq!(pr.base_branch, "main");
        assert_eq!(pr.head_branch, "feature/test");
        assert_eq!(pr.author, "dev1");
    }

    #[test]
    fn test_pr_info_deserialization_plain_author() {
        // Test deserialization with a plain string author (non-gh format)
        let json = r#"{
            "number": 10,
            "title": "Simple PR",
            "body": "",
            "state": "MERGED",
            "base_branch": "master",
            "head_branch": "patch-1",
            "author": "simpleuser",
            "created_at": "2026-01-01T00:00:00Z",
            "url": "https://github.com/x/y/pull/10"
        }"#;

        let pr: PrInfo = serde_json::from_str(json).unwrap();
        assert_eq!(pr.number, 10);
        assert_eq!(pr.author, "simpleuser");
        assert_eq!(pr.state, "MERGED");
    }
}
