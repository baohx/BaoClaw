//! Data types for Git integration module.
//!
//! Defines the core structures used across PR management, branch operations,
//! conflict resolution, and commit management.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Custom deserializer for the `author` field of PrInfo.
///
/// The `gh` CLI outputs author as `{"login":"dev1"}`, but we want just the
/// login name as a plain string. This handles both formats:
/// - `{"login": "dev1"}` (GitHub CLI) → `"dev1"`
/// - `{"username": "dev2"}` (GitLab CLI) → `"dev2"`
/// - `"dev1"` (plain string) → `"dev1"`
fn deserialize_author<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Object(map) => {
            if let Some(login) = map.get("login").and_then(|v| v.as_str()) {
                Ok(login.to_string())
            } else if let Some(username) = map.get("username").and_then(|v| v.as_str()) {
                Ok(username.to_string())
            } else {
                // Fallback: JSON-serialize the object
                Ok(serde_json::to_string(&map).unwrap_or_default())
            }
        }
        _ => Ok(value.to_string()),
    }
}

/// Custom serializer for `author` — always serializes as a plain string.
fn serialize_author<S>(author: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(author)
}

/// Information about a GitHub/GitLab pull request.
#[derive(Serialize, Clone, Debug)]
pub struct PrInfo {
    /// PR number (e.g. 123)
    pub number: u32,
    /// PR title
    pub title: String,
    /// PR description/body
    pub body: String,
    /// Current state: "OPEN", "CLOSED", "MERGED"
    pub state: String,
    /// Base branch (target, e.g. "main")
    pub base_branch: String,
    /// Head branch (source, e.g. "feature/auth")
    pub head_branch: String,
    /// Author username (extracted from gh CLI `author{login}` or GitLab `author{username}`)
    #[serde(deserialize_with = "deserialize_author", serialize_with = "serialize_author")]
    pub author: String,
    /// ISO-8601 creation timestamp
    pub created_at: String,
    /// PR URL
    pub url: String,
}

// Manual Deserialize impl needed because #[derive(Deserialize)] doesn't
// compose well with the custom serde attributes above, so we do it by hand.
impl<'de> Deserialize<'de> for PrInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PrInfoHelper {
            number: u32,
            title: String,
            body: String,
            state: String,
            #[serde(alias = "baseRefName")]
            base_branch: String,
            #[serde(alias = "headRefName")]
            head_branch: String,
            #[serde(deserialize_with = "deserialize_author")]
            author: String,
            #[serde(alias = "createdAt")]
            created_at: String,
            url: String,
        }

        let helper = PrInfoHelper::deserialize(deserializer)?;
        Ok(PrInfo {
            number: helper.number,
            title: helper.title,
            body: helper.body,
            state: helper.state,
            base_branch: helper.base_branch,
            head_branch: helper.head_branch,
            author: helper.author,
            created_at: helper.created_at,
            url: helper.url,
        })
    }
}

/// Information about a git branch.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BranchInfo {
    /// Branch name
    pub name: String,
    /// Whether this is the currently checked-out branch
    pub is_current: bool,
    /// Commits ahead of remote tracking branch
    pub ahead: u32,
    /// Commits behind remote tracking branch
    pub behind: u32,
    /// Abbreviated hash of the last commit on this branch
    pub last_commit: String,
    /// Subject line of the last commit on this branch
    pub last_commit_msg: String,
}

/// Information about a merge/rebase conflict on a file.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConflictInfo {
    /// File path with conflicts
    pub file: String,
    /// Content from "our" side (current branch during merge, or the branch being rebased)
    pub ours: String,
    /// Content from "their" side (incoming branch during merge, or the upstream during rebase)
    pub theirs: String,
    /// Whether this conflict has been resolved
    pub resolved: bool,
}

/// Information about a git commit.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitInfo {
    /// Full or abbreviated commit hash
    pub hash: String,
    /// Commit message
    pub message: String,
    /// Author name
    pub author: String,
    /// ISO-8601 timestamp of the commit
    pub timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pr_info_serde_roundtrip() {
        let pr = PrInfo {
            number: 42,
            title: "Fix login bug".into(),
            body: "Fixes #100".into(),
            state: "OPEN".into(),
            base_branch: "main".into(),
            head_branch: "fix/login".into(),
            author: "developer".into(),
            created_at: "2026-01-15T10:30:00Z".into(),
            url: "https://github.com/org/repo/pull/42".into(),
        };
        let json = serde_json::to_string(&pr).unwrap();
        let decoded: PrInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.number, 42);
        assert_eq!(decoded.title, "Fix login bug");
        assert_eq!(decoded.state, "OPEN");
    }

    #[test]
    fn test_branch_info_defaults() {
        let branch = BranchInfo {
            name: "feature/auth".into(),
            is_current: true,
            ahead: 3,
            behind: 0,
            last_commit: "abc1234".into(),
            last_commit_msg: "Add OAuth flow".into(),
        };
        assert!(branch.is_current);
        assert_eq!(branch.ahead, 3);
        assert_eq!(branch.behind, 0);
        let cloned = branch.clone();
        assert_eq!(cloned.name, branch.name);
    }

    #[test]
    fn test_conflict_info_resolved_flag() {
        let mut conflict = ConflictInfo {
            file: "src/auth.rs".into(),
            ours: "our content".into(),
            theirs: "their content".into(),
            resolved: false,
        };
        assert!(!conflict.resolved);
        conflict.resolved = true;
        assert!(conflict.resolved);

        let json = serde_json::to_string(&conflict).unwrap();
        let decoded: ConflictInfo = serde_json::from_str(&json).unwrap();
        assert!(decoded.resolved);
    }

    #[test]
    fn test_commit_info_fields() {
        let commit = CommitInfo {
            hash: "abc123def456".into(),
            message: "feat: add login endpoint".into(),
            author: "dev <dev@example.com>".into(),
            timestamp: "2026-01-15T10:00:00Z".into(),
        };
        assert!(commit.hash.len() > 7);
        assert!(commit.message.contains("login"));
        assert!(commit.author.contains("dev"));
    }
}
