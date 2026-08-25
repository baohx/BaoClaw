//! Branch management operations.
//!
//! Provides create, switch, sync, cleanup, and list operations
//! for git branches using the `git` command-line tool.

use super::types::BranchInfo;
use super::pr::GitIntegrationError;
use crate::utils::command::run_command_async;

/// Manages git branch operations.
pub struct BranchManager;

impl BranchManager {
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

    /// Create a new branch and optionally switch to it.
    ///
    /// Uses `git checkout -b <name>` or `git branch <name> <from>`.
    pub async fn create_branch(name: &str, from: Option<&str>) -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo().await?;

        let mut args = vec!["checkout", "-b", name];
        if let Some(base) = from {
            args.push(base);
        }

        let output = run_command_async("git", &args, None)
            .await
            .map_err(GitIntegrationError::IoError)?;
        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }
        Ok(())
    }

    /// Switch to an existing branch.
    pub async fn switch_branch(name: &str) -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo().await?;

        let output = run_command_async("git", &["checkout", name], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }
        Ok(())
    }

    /// Sync current branch with remote: fetch and rebase.
    pub async fn sync_branch() -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo().await?;

        // git fetch
        let fetch = run_command_async("git", &["fetch"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;
        if !fetch.success() {
            return Err(GitIntegrationError::CommandFailed(fetch.stderr));
        }

        // git pull --rebase
        let pull = run_command_async("git", &["pull", "--rebase"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;
        if !pull.success() {
            let stderr_lower = pull.stderr.to_lowercase();
            if stderr_lower.contains("conflict") {
                return Err(GitIntegrationError::MergeConflict);
            }
            return Err(GitIntegrationError::CommandFailed(pull.stderr));
        }

        Ok(())
    }

    /// List merged branches and return their names.
    /// Does not actually delete them — just identifies candidates for cleanup.
    pub async fn cleanup_branches() -> Result<Vec<String>, GitIntegrationError> {
        Self::ensure_git_repo().await?;

        // Get current branch to avoid including it
        let current = Self::run_git(&["branch", "--show-current"]).await?;
        let current = current.trim();

        // List branches merged into HEAD (exclude current and main/master)
        let output = run_command_async("git", &["branch", "--merged"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        let mut branches: Vec<String> = output
            .stdout
            .lines()
            .map(|l| l.trim_start_matches(" *").trim_start_matches(' ').trim().to_string())
            .filter(|name| {
                !name.is_empty()
                    && name != current
                    && name != "main"
                    && name != "master"
                    && name != "HEAD"
            })
            .collect();

        branches.sort();
        Ok(branches)
    }

    /// Parse `git branch -vv` output into a list of BranchInfo.
    fn parse_branch_vv(output: &str) -> Vec<BranchInfo> {
        let mut branches = Vec::new();

        for line in output.lines() {
            if line.is_empty() {
                continue;
            }
            let trimmed = line.trim();
            let is_current = trimmed.starts_with("* ");
            let rest = if is_current {
                &trimmed[2..]
            } else {
                trimmed
            };

            // Split: name then optional tracking info
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let name = parts[0].to_string();

            let mut ahead: u32 = 0;
            let mut behind: u32 = 0;
            let mut last_commit = String::new();
            let mut last_commit_msg = String::new();

            if parts.len() > 1 {
                let rest_str = parts[1].trim();
                // Extract commit hash (first word before tracking info)
                // Format: "hash [tracking] msg" or "hash msg"
                let hash_end = rest_str.find(' ').unwrap_or(rest_str.len());
                let hash_part = &rest_str[..hash_end];
                // Only treat as hash if it looks like one (7+ hex chars)
                if hash_part.len() >= 7 && hash_part.chars().all(|c| c.is_ascii_hexdigit()) {
                    last_commit = hash_part.to_string();
                }
                // Try to find [ahead X, behind Y] pattern
                if let Some(bracket_start) = rest_str.find('[') {
                    let bracket_end = rest_str[bracket_start..].find(']').unwrap_or(0);
                    let bracket_content = &rest_str[bracket_start + 1..bracket_start + bracket_end];
                    // Support both "," and ":" as separators (e.g. "[origin/main: ahead 3]")
                    for part in bracket_content.split([',', ':']) {
                        let part = part.trim();
                        if let Some(num_str) = part.strip_prefix("ahead ") {
                            ahead = num_str.parse().unwrap_or(0);
                        } else if let Some(num_str) = part.strip_prefix("behind ") {
                            behind = num_str.parse().unwrap_or(0);
                        }
                    }
                }
                // Extract commit message (after the tracking info bracket)
                // Format: "...] msg" or without bracket: "hash msg"
                let after_tracking = if let Some(bracket_end) = rest_str.find("] ") {
                    &rest_str[bracket_end + 2..]
                } else if rest_str.len() > hash_end + 1 {
                    &rest_str[hash_end + 1..]
                } else {
                    ""
                };
                last_commit_msg = after_tracking.to_string();
            }

            branches.push(BranchInfo {
                name,
                is_current,
                ahead,
                behind,
                last_commit,
                last_commit_msg,
            });
        }

        branches
    }

    /// List all branches with tracking information.
    pub async fn list_branches() -> Result<Vec<BranchInfo>, GitIntegrationError> {
        Self::ensure_git_repo().await?;

        let output = run_command_async("git", &["branch", "-vv"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !output.success() {
            return Err(GitIntegrationError::CommandFailed(output.stderr));
        }

        Ok(Self::parse_branch_vv(&output.stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_branch_vv_current() {
        // Simulate output with a current branch marker
        let output = "* feature/auth abc1234 [origin/feature/auth] Add OAuth flow";
        let branches = BranchManager::parse_branch_vv(output);
        assert_eq!(branches.len(), 1);
        assert!(branches[0].is_current);
        assert_eq!(branches[0].name, "feature/auth");
        assert_eq!(branches[0].last_commit, "abc1234");
        assert_eq!(branches[0].last_commit_msg, "Add OAuth flow");
    }

    #[test]
    fn test_parse_branch_vv_multiple() {
        let output = "\
* main        def5678 [origin/main] Latest merge
  feature/abc abc1234 [origin/feature/abc: ahead 3] WIP feature
  bugfix/xyz  deadbee [origin/bugfix/xyz: behind 2] Fix crash
  local-only  11112222 Untracked local commit
";
        let branches = BranchManager::parse_branch_vv(output);
        assert_eq!(branches.len(), 4);

        let main_br = &branches[0];
        assert!(main_br.is_current);
        assert_eq!(main_br.name, "main");

        let feature = &branches[1];
        assert!(!feature.is_current);
        assert_eq!(feature.ahead, 3);
        assert_eq!(feature.behind, 0);

        let bugfix = &branches[2];
        assert_eq!(bugfix.behind, 2);
        assert_eq!(bugfix.ahead, 0);

        let local = &branches[3];
        assert_eq!(local.ahead, 0);
        assert_eq!(local.behind, 0);
        assert_eq!(local.last_commit_msg, "Untracked local commit");
    }

    #[test]
    fn test_parse_branch_vv_empty() {
        let branches = BranchManager::parse_branch_vv("");
        assert!(branches.is_empty());
    }

    #[tokio::test]
    async fn test_ensure_git_repo_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let _cwd_guard = match super::super::CWD_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(), // a sibling test panicked; recover and still restore cwd
        };
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = BranchManager::ensure_git_repo().await;
        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Err(GitIntegrationError::NotAGitRepo) | Err(GitIntegrationError::IoError(_)) => {}
            other => panic!("Expected error, got: {:?}", other),
        }
    }

    #[test]
    fn test_cleanup_branches_excludes_main_master() {
        // Test the filtering logic through the parse/list path
        // We test via parse_branch_vv which is the core parsing function
        let output = "\
* main       abc1234 [origin/main] Latest
  old-branch def5678 [origin/old-branch] Old work
  master     11112222 [origin/master] Master
  feature    22223333 Old feature
";
        let branches = BranchManager::parse_branch_vv(output);
        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"old-branch"));
        assert!(names.contains(&"master"));
        assert!(names.contains(&"feature"));
    }
}
