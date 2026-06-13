//! Merge/rebase conflict detection and resolution.
//!
//! Detects conflicts from ongoing merge or rebase operations,
//! and provides resolution strategies (ours/theirs/abort).

use std::path::Path;
use std::process::Command;

use super::types::ConflictInfo;
use super::pr::GitIntegrationError;

/// Manages merge/rebase conflict detection and resolution.
pub struct ConflictResolver;

impl ConflictResolver {
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

    /// Detect currently conflicted files from ongoing merge/rebase.
    ///
    /// Uses `git diff --name-only --diff-filter=U` to list unmerged files,
    /// then extracts ours/theirs content from each file.
    pub fn detect_conflicts() -> Result<Vec<ConflictInfo>, GitIntegrationError> {
        Self::ensure_git_repo()?;

        // Find unmerged files
        let output = Command::new("git")
            .args(["diff", "--name-only", "--diff-filter=U"])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        // If the command fails, there may be no merge/rebase in progress
        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8(output.stdout)?;
        let files: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

        let mut conflicts = Vec::new();
        for file in &files {
            let ours = Self::get_conflict_content(file, "ours").unwrap_or_default();
            let theirs = Self::get_conflict_content(file, "theirs").unwrap_or_default();
            conflicts.push(ConflictInfo {
                file: file.to_string(),
                ours,
                theirs,
                resolved: false,
            });
        }

        Ok(conflicts)
    }

    /// Get content of a specific side of a conflicted file.
    fn get_conflict_content(file: &str, stage: &str) -> Result<String, GitIntegrationError> {
        let stage_num = if stage == "ours" { "2" } else { "3" };

        // Try `git show :<stage>:<file>` to get the staged version
        let output = Command::new("git")
            .args(["show", &format!(":{}:{}", stage_num, file)])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if output.status.success() {
            return Ok(String::from_utf8(output.stdout)?);
        }

        // Fallback: parse conflict markers from the working-tree file
        Self::parse_conflict_markers(file, stage)
    }

    /// Parse conflict markers (<<<<<<<, =======, >>>>>>>) to extract content.
    fn parse_conflict_markers(file: &str, stage: &str) -> Result<String, GitIntegrationError> {
        let path = Path::new(file);
        if !path.exists() {
            return Ok(String::new());
        }
        let content = std::fs::read_to_string(path).map_err(GitIntegrationError::IoError)?;
        let mut result = String::new();
        let mut in_ours = false;
        let mut in_theirs = false;

        for line in content.lines() {
            if line.starts_with("<<<<<<<") {
                in_ours = true;
                in_theirs = false;
                continue;
            } else if line.starts_with("=======") {
                in_ours = false;
                in_theirs = true;
                continue;
            } else if line.starts_with(">>>>>>>") {
                in_ours = false;
                in_theirs = false;
                continue;
            }

            match stage {
                "ours" if in_ours => {
                    result.push_str(line);
                    result.push('\n');
                }
                "theirs" if in_theirs => {
                    result.push_str(line);
                    result.push('\n');
                }
                _ => {}
            }
        }

        Ok(result)
    }

    /// Resolve a conflict by choosing one side.
    ///
    /// # Arguments
    /// * `strategy` - "ours" to keep current branch changes, "theirs" to accept incoming changes
    pub fn resolve_strategy(strategy: &str) -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo()?;

        let flag = match strategy {
            "ours" => "--ours",
            "theirs" => "--theirs",
            _ => {
                return Err(GitIntegrationError::CommandFailed(format!(
                    "Unknown strategy: {}. Use 'ours' or 'theirs'.",
                    strategy
                )));
            }
        };

        // Stage all conflicted files with the chosen strategy
        let checkout = Command::new("git")
            .args(["checkout", flag, "."])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !checkout.status.success() {
            let stderr = String::from_utf8_lossy(&checkout.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        // Add the resolved files
        let add = Command::new("git")
            .args(["add", "."])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if !add.status.success() {
            let stderr = String::from_utf8_lossy(&add.stderr);
            return Err(GitIntegrationError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Abort an ongoing merge or rebase operation.
    pub fn abort_operation() -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo()?;

        // Try rebase abort first
        let rebase = Command::new("git")
            .args(["rebase", "--abort"])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if rebase.status.success() {
            return Ok(());
        }

        // Try merge abort
        let merge = Command::new("git")
            .args(["merge", "--abort"])
            .output()
            .map_err(GitIntegrationError::IoError)?;

        if merge.status.success() {
            return Ok(());
        }

        // Neither worked — check if we're actually in a conflicted state
        let rebase_stderr = String::from_utf8_lossy(&rebase.stderr);
        let merge_stderr = String::from_utf8_lossy(&merge.stderr);

        if rebase_stderr.contains("no rebase") && merge_stderr.contains("no merge") {
            return Err(GitIntegrationError::CommandFailed(
                "No rebase or merge in progress to abort".into(),
            ));
        }

        Err(GitIntegrationError::CommandFailed(format!(
            "Failed to abort: rebase: {}, merge: {}",
            rebase_stderr.trim(),
            merge_stderr.trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_conflict_markers_ours() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("conflict.txt");
        let content = "\
line1
<<<<<<< HEAD
our change
=======
their change
>>>>>>> feature
line5
";
        std::fs::write(&file_path, content).unwrap();

        let ours = ConflictResolver::parse_conflict_markers(
            file_path.to_str().unwrap(),
            "ours",
        )
        .unwrap();
        assert_eq!(ours.trim(), "our change");
    }

    #[test]
    fn test_parse_conflict_markers_theirs() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("conflict.txt");
        let content = "\
line1
<<<<<<< HEAD
our change
=======
their change
>>>>>>> feature
line5
";
        std::fs::write(&file_path, content).unwrap();

        let theirs = ConflictResolver::parse_conflict_markers(
            file_path.to_str().unwrap(),
            "theirs",
        )
        .unwrap();
        assert_eq!(theirs.trim(), "their change");
    }

    #[test]
    fn test_parse_conflict_markers_nonexistent_file() {
        let result = ConflictResolver::parse_conflict_markers("/nonexistent/file.txt", "ours");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_resolve_strategy_invalid() {
        let result = ConflictResolver::resolve_strategy("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_conflicts_no_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = ConflictResolver::detect_conflicts();
        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Err(GitIntegrationError::NotAGitRepo) | Err(GitIntegrationError::IoError(_)) => {}
            other => panic!("Expected error, got: {:?}", other),
        }
    }
}
