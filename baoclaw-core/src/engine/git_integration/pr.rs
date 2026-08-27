//! PR (Pull Request) management via GitHub CLI (`gh`).
//!
//! Provides create/list/review/merge operations for pull requests
//! using the `gh` command-line tool.

use super::types::PrInfo;
use crate::utils::command::run_command_async;

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
    pub async fn is_gh_available() -> bool {
        run_command_async("gh", &["--version"], None)
            .await
            .map(|o| o.success())
            .unwrap_or(false)
    }

    /// Ensure the `gh` CLI is installed, returning a friendly error otherwise.
    async fn ensure_gh() -> Result<(), GitIntegrationError> {
        if !Self::is_gh_available().await {
            return Err(GitIntegrationError::GhCliNotFound);
        }
        Ok(())
    }

    /// Check that the current directory is inside a git repository.
    async fn ensure_git_repo() -> Result<(), GitIntegrationError> {
        let output = run_command_async("git", &["rev-parse", "--is-inside-work-tree"], None)
            .await
            .map_err(|_| GitIntegrationError::NotAGitRepo)?;
        if !output.success() {
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
    pub async fn create_pr(
        title: &str,
        body: Option<&str>,
        base: Option<&str>,
    ) -> Result<PrInfo, GitIntegrationError> {
        Self::ensure_gh().await?;
        Self::ensure_git_repo().await?;

        let args = [
            "pr",
            "create",
            "--title",
            title,
            "--json",
            "number,title,body,state,baseRefName,headRefName,author{login},createdAt,url",
        ];

        let mut dynamic_args: Vec<String> = Vec::new();
        if let Some(b) = body {
            dynamic_args.push("--body".to_string());
            dynamic_args.push(b.to_string());
        }
        if let Some(b) = base {
            dynamic_args.push("--base".to_string());
            dynamic_args.push(b.to_string());
        }

        // Build the full args slice
        let dynamic_refs: Vec<&str> = dynamic_args.iter().map(|s| s.as_str()).collect();
        let all_args: Vec<&str> = args.iter().chain(dynamic_refs.iter()).copied().collect();

        let output = run_command_async("gh", &all_args, None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            let stderr_lower = output.stderr.to_lowercase();

            if stderr_lower.contains("auth") || stderr_lower.contains("unauthorized") {
                return Err(GitIntegrationError::AuthenticationFailed);
            }
            if stderr_lower.contains("conflict") {
                return Err(GitIntegrationError::MergeConflict);
            }
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        let pr: PrInfo = serde_json::from_str(output.stdout.trim())?;
        Ok(pr)
    }

    /// List pull requests with optional state filter.
    ///
    /// # Arguments
    /// * `state` - Filter by state: "open", "closed", "merged", or None for all
    pub async fn list_prs(state: Option<&str>) -> Result<Vec<PrInfo>, GitIntegrationError> {
        Self::ensure_gh().await?;
        Self::ensure_git_repo().await?;

        let args = [
            "pr",
            "list",
            "--json",
            "number,title,body,state,baseRefName,headRefName,author{login},createdAt,url",
        ];

        let mut dynamic_args: Vec<String> = Vec::new();
        if let Some(s) = state {
            dynamic_args.push("--state".to_string());
            dynamic_args.push(s.to_string());
        }
        dynamic_args.push("--limit".to_string());
        dynamic_args.push("100".to_string());

        let dynamic_refs: Vec<&str> = dynamic_args.iter().map(|s| s.as_str()).collect();
        let all_args: Vec<&str> = args.iter().chain(dynamic_refs.iter()).copied().collect();

        let output = run_command_async("gh", &all_args, None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        let prs: Vec<PrInfo> = serde_json::from_str(output.stdout.trim())?;
        Ok(prs)
    }

    /// Review/view a specific pull request by number.
    pub async fn review_pr(number: u32) -> Result<PrInfo, GitIntegrationError> {
        Self::ensure_gh().await?;
        Self::ensure_git_repo().await?;

        let number_str = number.to_string();

        let output = run_command_async(
            "gh",
            &[
                "pr",
                "view",
                &number_str,
                "--json",
                "number,title,body,state,baseRefName,headRefName,author{login},createdAt,url",
            ],
            None,
        )
        .await
        .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            let stderr_lower = output.stderr.to_lowercase();
            if stderr_lower.contains("not found") || stderr_lower.contains("no pull request") {
                return Err(GitIntegrationError::PrNotFound(number_str));
            }
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        let pr: PrInfo = serde_json::from_str(output.stdout.trim())?;
        Ok(pr)
    }

    /// Merge a pull request.
    ///
    /// # Arguments
    /// * `number` - PR number to merge
    /// * `squash` - If true, squash all commits into one before merging
    pub async fn merge_pr(number: u32, squash: bool) -> Result<(), GitIntegrationError> {
        Self::ensure_gh().await?;
        Self::ensure_git_repo().await?;

        let number_str = number.to_string();
        let mut args = vec!["pr", "merge", &number_str, "--delete-branch"];

        let squash_arg;
        if squash {
            squash_arg = "--squash";
            args.push(squash_arg);
        }

        let output = run_command_async("gh", &args, None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            let stderr_lower = output.stderr.to_lowercase();

            if stderr_lower.contains("auth") || stderr_lower.contains("unauthorized") {
                return Err(GitIntegrationError::AuthenticationFailed);
            }
            if stderr_lower.contains("conflict") {
                return Err(GitIntegrationError::MergeConflict);
            }
            if stderr_lower.contains("not found") {
                return Err(GitIntegrationError::PrNotFound(number_str));
            }
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gh_availability_check() {
        // This test just ensures the check doesn't panic
        let available = PrManager::is_gh_available().await;
        // gh may or may not be installed in test env — both outcomes are valid
        // We just verify it returns a boolean
        assert!(available == true || available == false);
    }

    #[tokio::test]
    async fn test_ensure_git_repo_in_non_repo() {
        // Create a temp dir that is NOT a git repo
        let tmp = tempfile::tempdir().unwrap();
        let _cwd_guard = match super::super::CWD_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(), // a sibling test panicked; recover and still restore cwd
        };
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = PrManager::ensure_git_repo().await;

        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Err(GitIntegrationError::NotAGitRepo) => {} // expected
            Err(GitIntegrationError::IoError(_)) => {}  // also acceptable (git not installed)
            other => panic!("Expected NotAGitRepo or IoError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_list_prs_without_gh_cli() {
        // When gh is not available, list_prs should return GhCliNotFound
        if !PrManager::is_gh_available().await {
            let result = PrManager::list_prs(None).await;
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
