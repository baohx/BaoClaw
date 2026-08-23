//! Merge/rebase conflict detection and resolution.
//!
//! Detects conflicts from ongoing merge or rebase operations,
//! and provides resolution strategies (ours/theirs/abort).

use std::path::Path;

use super::types::ConflictInfo;
use super::pr::GitIntegrationError;
use crate::utils::command::run_command_async;

/// Manages merge/rebase conflict detection and resolution.
pub struct ConflictResolver;

impl ConflictResolver {
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

    /// Detect currently conflicted files from ongoing merge/rebase.
    ///
    /// Uses `git diff --name-only --diff-filter=U` to list unmerged files,
    /// then extracts ours/theirs content from each file.
    pub async fn detect_conflicts() -> Result<Vec<ConflictInfo>, GitIntegrationError> {
        Self::ensure_git_repo().await?;

        // Find unmerged files
        let output = run_command_async("git", &["diff", "--name-only", "--diff-filter=U"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        // If the command fails, there may be no merge/rebase in progress
        if !output.success() {
            return Ok(Vec::new());
        }

        let files: Vec<&str> = output.stdout.lines().filter(|l| !l.is_empty()).collect();

        let mut conflicts = Vec::new();
        for file in &files {
            let ours = Self::get_conflict_content(file, "ours").await.unwrap_or_default();
            let theirs = Self::get_conflict_content(file, "theirs").await.unwrap_or_default();
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
    async fn get_conflict_content(file: &str, stage: &str) -> Result<String, GitIntegrationError> {
        let stage_num = if stage == "ours" { "2" } else { "3" };

        // Try `git show :<stage>:<file>` to get the staged version
        let show_arg = format!(":{}:{}", stage_num, file);
        let output = run_command_async("git", &["show", &show_arg], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if output.success() {
            return Ok(output.stdout);
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
    pub async fn resolve_strategy(strategy: &str) -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo().await?;

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
        let checkout = run_command_async("git", &["checkout", flag, "."], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !checkout.success() {
            return Err(GitIntegrationError::CommandFailed(checkout.stderr));
        }

        // Add the resolved files
        let add = run_command_async("git", &["add", "."], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if !add.success() {
            return Err(GitIntegrationError::CommandFailed(add.stderr));
        }

        Ok(())
    }

    /// Abort an ongoing merge or rebase operation.
    pub async fn abort_operation() -> Result<(), GitIntegrationError> {
        Self::ensure_git_repo().await?;

        // Try rebase abort first
        let rebase = run_command_async("git", &["rebase", "--abort"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if rebase.success() {
            return Ok(());
        }

        // Try merge abort
        let merge = run_command_async("git", &["merge", "--abort"], None)
            .await
            .map_err(GitIntegrationError::IoError)?;

        if merge.success() {
            return Ok(());
        }

        // Neither worked — check if we're actually in a conflicted state
        let rebase_stderr = &rebase.stderr;
        let merge_stderr = &merge.stderr;

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

    #[tokio::test]
    async fn test_resolve_strategy_invalid() {
        let result = ConflictResolver::resolve_strategy("invalid").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_detect_conflicts_no_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let _cwd_guard = match super::super::CWD_LOCK.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(), // a sibling test panicked; recover and still restore cwd
        };
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let result = ConflictResolver::detect_conflicts().await;
        std::env::set_current_dir(&cwd).unwrap();

        match result {
            Err(GitIntegrationError::NotAGitRepo) | Err(GitIntegrationError::IoError(_)) => {}
            other => panic!("Expected error, got: {:?}", other),
        }
    }
}
