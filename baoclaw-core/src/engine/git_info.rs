use std::path::Path;
use std::process::Command;

/// Git repository information.
#[derive(Clone, Debug, PartialEq)]
pub struct GitInfo {
    pub branch: Option<String>,
    pub has_changes: bool,
    pub staged_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub untracked_files: Vec<String>,
}

/// Collect git information for the given working directory.
///
/// Returns `None` if the directory is not inside a git repository
/// or if the `git` binary is unavailable.
pub fn get_git_info(cwd: &Path) -> Option<GitInfo> {
    // Check if inside a git work tree
    let inside = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !inside.status.success() {
        return None;
    }

    // Get current branch name
    let branch = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Get porcelain status
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .ok()?;
    let status_text = String::from_utf8_lossy(&status_output.stdout);

    let (staged, modified, untracked) = parse_porcelain_status(&status_text);

    Some(GitInfo {
        branch,
        has_changes: !staged.is_empty() || !modified.is_empty() || !untracked.is_empty(),
        staged_files: staged,
        modified_files: modified,
        untracked_files: untracked,
    })
}

/// Parse `git status --porcelain` output into (staged, modified, untracked) file lists.
///
/// Porcelain format: two-character status prefix followed by a space and the filename.
///   - Index char (position 0): staging area status
///   - Worktree char (position 1): working tree status
///   - `?` in index position means untracked
///   - Non-space, non-`?` in index position means staged
///   - `M` or `D` in worktree position means modified/deleted in working tree
pub fn parse_porcelain_status(output: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut staged = Vec::new();
    let mut modified = Vec::new();
    let mut untracked = Vec::new();

    for line in output.lines() {
        if line.len() < 4 {
            continue;
        }
        let bytes = line.as_bytes();
        let index_char = bytes[0] as char;
        let worktree_char = bytes[1] as char;
        let file = line[3..].to_string();

        if index_char == '?' {
            untracked.push(file);
        } else {
            if index_char != ' ' {
                staged.push(file.clone());
            }
            if worktree_char == 'M' || worktree_char == 'D' {
                modified.push(file);
            }
        }
    }

    (staged, modified, untracked)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_porcelain_status tests ---

    #[test]
    fn test_parse_empty_output() {
        let (staged, modified, untracked) = parse_porcelain_status("");
        assert!(staged.is_empty());
        assert!(modified.is_empty());
        assert!(untracked.is_empty());
    }

    #[test]
    fn test_parse_staged_file() {
        // "M " means staged modification, clean worktree
        let (staged, modified, untracked) = parse_porcelain_status("M  src/main.rs\n");
        assert_eq!(staged, vec!["src/main.rs"]);
        assert!(modified.is_empty());
        assert!(untracked.is_empty());
    }

    #[test]
    fn test_parse_modified_file() {
        // " M" means unstaged modification
        let (staged, modified, untracked) = parse_porcelain_status(" M src/lib.rs\n");
        assert!(staged.is_empty());
        assert_eq!(modified, vec!["src/lib.rs"]);
        assert!(untracked.is_empty());
    }

    #[test]
    fn test_parse_untracked_file() {
        let (staged, modified, untracked) = parse_porcelain_status("?? new_file.txt\n");
        assert!(staged.is_empty());
        assert!(modified.is_empty());
        assert_eq!(untracked, vec!["new_file.txt"]);
    }

    #[test]
    fn test_parse_staged_and_modified() {
        // "MM" means staged AND has unstaged modifications
        let (staged, modified, _) = parse_porcelain_status("MM src/both.rs\n");
        assert_eq!(staged, vec!["src/both.rs"]);
        assert_eq!(modified, vec!["src/both.rs"]);
    }

    #[test]
    fn test_parse_added_file() {
        // "A " means newly added to index
        let (staged, modified, untracked) = parse_porcelain_status("A  new.rs\n");
        assert_eq!(staged, vec!["new.rs"]);
        assert!(modified.is_empty());
        assert!(untracked.is_empty());
    }

    #[test]
    fn test_parse_deleted_in_worktree() {
        // " D" means deleted in worktree but not staged
        let (staged, modified, untracked) = parse_porcelain_status(" D old.rs\n");
        assert!(staged.is_empty());
        assert_eq!(modified, vec!["old.rs"]);
        assert!(untracked.is_empty());
    }

    #[test]
    fn test_parse_deleted_staged() {
        // "D " means deletion staged
        let (staged, modified, untracked) = parse_porcelain_status("D  removed.rs\n");
        assert_eq!(staged, vec!["removed.rs"]);
        assert!(modified.is_empty());
        assert!(untracked.is_empty());
    }

    #[test]
    fn test_parse_mixed_status() {
        let output = "\
M  staged.rs
 M modified.rs
?? untracked.txt
A  added.rs
 D deleted.rs
MM both.rs
";
        let (staged, modified, untracked) = parse_porcelain_status(output);
        assert_eq!(staged, vec!["staged.rs", "added.rs", "both.rs"]);
        assert_eq!(modified, vec!["modified.rs", "deleted.rs", "both.rs"]);
        assert_eq!(untracked, vec!["untracked.txt"]);
    }

    #[test]
    fn test_parse_short_lines_skipped() {
        // Lines shorter than 4 chars should be skipped
        let (staged, modified, untracked) = parse_porcelain_status("M\n \nAB\n");
        assert!(staged.is_empty());
        assert!(modified.is_empty());
        assert!(untracked.is_empty());
    }

    // --- get_git_info integration tests ---

    #[test]
    fn test_get_git_info_non_git_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let result = get_git_info(tmp.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_get_git_info_in_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        // Initialize a git repo
        let init = Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output();
        if init.is_err() || !init.unwrap().status.success() {
            // git not available in test environment, skip
            return;
        }
        // Configure git user for the test repo
        let _ = Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output();

        let info = get_git_info(tmp.path());
        assert!(info.is_some());
        let info = info.unwrap();
        // New repo with no commits — branch may be empty or "main"/"master"
        assert!(!info.has_changes);
        assert!(info.staged_files.is_empty());
        assert!(info.modified_files.is_empty());
        assert!(info.untracked_files.is_empty());
    }

    #[test]
    fn test_get_git_info_with_untracked_file() {
        let tmp = tempfile::tempdir().unwrap();
        let init = Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output();
        if init.is_err() || !init.unwrap().status.success() {
            return;
        }
        let _ = Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output();

        // Create an untracked file
        std::fs::write(tmp.path().join("hello.txt"), "hello").unwrap();

        let info = get_git_info(tmp.path()).unwrap();
        assert!(info.has_changes);
        assert!(info.untracked_files.contains(&"hello.txt".to_string()));
    }

    // --- GitInfo struct tests ---

    #[test]
    fn test_git_info_default_values() {
        let info = GitInfo {
            branch: None,
            has_changes: false,
            staged_files: vec![],
            modified_files: vec![],
            untracked_files: vec![],
        };
        assert!(info.branch.is_none());
        assert!(!info.has_changes);
    }

    #[test]
    fn test_git_info_clone() {
        let info = GitInfo {
            branch: Some("main".to_string()),
            has_changes: true,
            staged_files: vec!["a.rs".to_string()],
            modified_files: vec![],
            untracked_files: vec![],
        };
        let cloned = info.clone();
        assert_eq!(info, cloned);
    }
}
