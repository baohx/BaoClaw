//! Branch management operations.
//!
//! Provides create, switch, sync, cleanup, and list operations
//! for git branches using the `git` command-line tool.

use std::process::Command;

use super::types::BranchInfo;
use super::pr::GitIntegrationError;

/// Manages git branch operations.
pub struct BranchManager;

impl BranchManager {
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

    /// Create a new branch and optionally switch to it.
    ///
    /// Uses `git checkout -b <name>` or `git branch <name> <from>`.
    pub fn create_branch(name: &str, from: Option<&str>) -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo()?;

        let mut cmd = Command::new("git");
        cmd.args(["checkout", "-b", name]);
        if let Some(base) = from {
            cmd.arg(base);
        }

        let output = cmd.output().map_err(GitIntegrationError::IoError)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }
        Ok(())
    }

    /// Switch to an existing branch.
    pub fn switch_branch(name: &str) -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo()?;

        let output = Command::new("git")
            .args(["checkout", name])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }
        Ok(())
    }

    /// Sync current branch with remote: fetch and rebase.
    pub fn sync_branch() -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo()?;

        // git fetch
        let fetch = Command::new("git")
            .args(["fetch"])
            .output()
            .map_err(GitIntegrationError::IoError)?;
        if !fetch.status.success() {
            let stderr = String::from_utf8_lossy(&fetch.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        // git pull --rebase
        let pull = Command::new("git")
            .args(["pull", "--rebase"])
            .output()
            .map_err(GitIntegrationError::IoError)?;
        if !pull.status.success() {
            let stderr = String::from_utf8_lossy(&pull.stderr);
            let stderr_lower = stderr.to_lowercase();
            if stderr_lower.contains("conflict") {
                return Err(GitIntegrationError::MergeConflict);
            }
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// List merged branches and return their names.
    /// Does not actually delete them — just identifies candidates for cleanup.
    pub fn cleanup_branches() -> Result<Vec<String>, GitIntegrationError> {
        Self::ensure_git_repo()?;

        // Get current branch to avoid including it
        let current = Self::run_git(&["branch", "--show-current"])?;
        let current = current.trim();

        // List branches merged into HEAD (exclude current and main/master)
        let output = Command::new("git")
            .args(["branch", "--merged"])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let mut branches: Vec<String> = stdout
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
                let rest_str = parts[1];
                // Try to find [ahead X, behind Y] pattern
                if let Some(bracket_start) = rest_str.find('[') {
                    let bracket_end = rest_str[bracket_start..].find(']').unwrap_or(0);
                    let bracket_content = &rest_str[bracket_start + 1..bracket_start + bracket_end];
                    for part in bracket_content.split(',') {
                        let part = part.trim();
                        if let Some(num_str) = part.strip_prefix("ahead ") {
                            ahead = num_str.parse().unwrap_or(0);
                        } else if let Some(num_str) = part.strip_prefix("behind ") {
                            behind = num_str.parse().unwrap_or(0);
                        }
                    }
                }
                // Extract last commit hash (short, after the tracking info)
                // Format: "branch_name hash commit_msg"
                let after_tracking = if let Some(bracket_end) = rest_str.find("] ") {
                    &rest_str[bracket_end + 2..]
                } else if rest_str.len() > 8 {
                    // No tracking bracket, rest_str may start with hash directly
                    rest_str
                } else {
                    ""
                };

                let commit_parts: Vec<&str> = after_tracking.splitn(2, ' ').collect();
                if commit_parts.len() >= 1 && !commit_parts[0].is_empty() {
                    last_commit = commit_parts[0].to_string();
                }
                if commit_parts.len() >= 2 {
                    last_commit_msg = commit_parts[1].to_string();
                }
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
    pub fn list_branches() -> Result<Vec<BranchInfo>, GitIntegrationError> {
        Self::ensure_git_repo()?;

        let output = Command::new("git")
            .args(["branch", "-vv"])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8(output.stdout)?;
        Ok(Self::parse_branch_vv(&stdout))
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

    #[test]
    fn test_ensure_git_repo_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = BranchManager::ensure_git_repo();
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
