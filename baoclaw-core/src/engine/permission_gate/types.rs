//! Data types for the interactive permission gate system.
//!
//! Defines permission rules, requests, decisions, and cache entries
//! used by the PermissionGate to control tool access.

use serde::{Deserialize, Serialize};

/// A permission rule that controls access to a tool or action.
///
/// Rules are matched against tool name, action, and optionally a target
/// pattern (e.g., file path glob). Rules can require user confirmation
/// or auto-deny dangerous operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionRule {
    /// Unique identifier for this rule.
    pub id: String,
    /// Human-readable description of what this rule does.
    pub description: String,
    /// The tool this rule applies to (e.g., "Bash", "FileWrite", "FileRead").
    pub tool: String,
    /// The action or command within the tool (e.g., "rm -rf", "npm install").
    /// Use "*" to match any action.
    pub action: String,
    /// Optional glob/regex pattern for matching targets (e.g., file paths, URLs).
    /// `None` means match any target.
    pub target_pattern: Option<String>,
    /// If true, the user must confirm before this action proceeds.
    pub require_confirmation: bool,
    /// If true, this action is automatically denied without prompting.
    pub auto_deny: bool,
}

impl PermissionRule {
    /// Create a new permission rule.
    pub fn new(id: &str, description: &str, tool: &str, action: &str) -> Self {
        Self {
            id: id.to_string(),
            description: description.to_string(),
            tool: tool.to_string(),
            action: action.to_string(),
            target_pattern: None,
            require_confirmation: false,
            auto_deny: false,
        }
    }

    /// Set the target pattern for this rule.
    pub fn with_target(mut self, pattern: &str) -> Self {
        self.target_pattern = Some(pattern.to_string());
        self
    }

    /// Mark this rule as requiring user confirmation.
    pub fn with_confirmation(mut self) -> Self {
        self.require_confirmation = true;
        self
    }

    /// Mark this rule for auto-deny.
    pub fn with_auto_deny(mut self) -> Self {
        self.auto_deny = true;
        self
    }

    /// Check if this rule's target pattern matches the given target.
    pub fn matches_target(&self, target: &str) -> bool {
        match &self.target_pattern {
            None => true, // No pattern means match everything
            Some(pattern) => {
                // Support simple glob-style patterns
                if pattern == "*" {
                    return true;
                }
                // Try glob matching (supports *, ?, [...])
                if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                    glob_pattern.matches(target)
                } else {
                    // Fallback: simple substring match
                    target.contains(pattern.as_str())
                }
            }
        }
    }

    /// Check if this rule matches the given tool and action.
    pub fn matches_tool_action(&self, tool: &str, action: &str) -> bool {
        if self.tool != "*" && self.tool != tool {
            return false;
        }
        if self.action != "*" {
            // Simple substring or prefix matching for actions
            if !action.starts_with(&self.action) && action != self.action {
                return false;
            }
        }
        true
    }
}

/// A permission request generated when a tool wants to perform an action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionRequest {
    /// Unique request identifier.
    pub id: String,
    /// The tool making the request (e.g., "Bash", "FileWrite").
    pub tool: String,
    /// The action being attempted (e.g., "rm -rf /tmp/test").
    pub action: String,
    /// The target of the action (e.g., file path, URL).
    pub target: String,
    /// Unix timestamp when the request was created.
    pub timestamp: u64,
}

impl PermissionRequest {
    /// Create a new permission request with a generated UUID.
    pub fn new(tool: &str, action: &str, target: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.to_string(),
            action: action.to_string(),
            target: target.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// The result of evaluating a permission request against rules and cache.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionDecision {
    /// The ID of the request this decision answers.
    pub request_id: String,
    /// What to do about the request.
    pub decision: DecisionType,
    /// The ID of the rule that triggered this decision (if any).
    pub rule_applied: Option<String>,
    /// Unix timestamp when this decision expires (None = never).
    pub expires_at: Option<u64>,
}

impl PermissionDecision {
    /// Create an allow decision.
    pub fn allow(request_id: &str) -> Self {
        Self {
            request_id: request_id.to_string(),
            decision: DecisionType::Allow,
            rule_applied: None,
            expires_at: None,
        }
    }

    /// Create a deny decision.
    pub fn deny(request_id: &str, reason: Option<&str>) -> Self {
        Self {
            request_id: request_id.to_string(),
            decision: DecisionType::Deny,
            rule_applied: reason.map(|s| s.to_string()),
            expires_at: None,
        }
    }

    /// Create an ask-user decision.
    pub fn ask_user(request_id: &str) -> Self {
        Self {
            request_id: request_id.to_string(),
            decision: DecisionType::AskUser,
            rule_applied: None,
            expires_at: None,
        }
    }

    /// Create a decision from a rule match.
    pub fn from_rule(request_id: &str, rule: &PermissionRule) -> Self {
        if rule.auto_deny {
            Self {
                request_id: request_id.to_string(),
                decision: DecisionType::Deny,
                rule_applied: Some(rule.id.clone()),
                expires_at: None,
            }
        } else if rule.require_confirmation {
            Self {
                request_id: request_id.to_string(),
                decision: DecisionType::AskUser,
                rule_applied: Some(rule.id.clone()),
                expires_at: None,
            }
        } else {
            Self {
                request_id: request_id.to_string(),
                decision: DecisionType::Allow,
                rule_applied: Some(rule.id.clone()),
                expires_at: None,
            }
        }
    }
}

/// The type of decision for a permission request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DecisionType {
    /// Automatically allow the action.
    Allow,
    /// Automatically deny the action.
    Deny,
    /// Prompt the user for a decision.
    AskUser,
    /// Allow this specific request once (not cached).
    AllowOnce,
    /// Allow for the duration of the session (cached until restart).
    AllowSession,
    /// Allow permanently (cached indefinitely).
    AllowPermanent,
}

impl DecisionType {
    /// Returns true if this decision allows the action.
    pub fn is_allowed(&self) -> bool {
        matches!(
            self,
            DecisionType::Allow | DecisionType::AllowOnce | DecisionType::AllowSession | DecisionType::AllowPermanent
        )
    }

    /// Returns true if this decision requires user input.
    pub fn requires_user_input(&self) -> bool {
        matches!(self, DecisionType::AskUser)
    }
}

impl std::fmt::Display for DecisionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecisionType::Allow => write!(f, "allow"),
            DecisionType::Deny => write!(f, "deny"),
            DecisionType::AskUser => write!(f, "ask"),
            DecisionType::AllowOnce => write!(f, "once"),
            DecisionType::AllowSession => write!(f, "session"),
            DecisionType::AllowPermanent => write!(f, "always"),
        }
    }
}

/// A cached permission grant entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheEntry {
    /// The permission grant (AllowSession, AllowPermanent, etc.).
    pub grant: DecisionType,
    /// The tool this grant applies to.
    pub tool: String,
    /// The action this grant applies to.
    pub action: String,
    /// The target this grant applies to.
    pub target: String,
    /// Unix timestamp when this grant expires (None = permanent, never expires).
    pub expires_at: Option<u64>,
}

impl CacheEntry {
    /// Create a new cache entry.
    pub fn new(
        grant: DecisionType,
        tool: &str,
        action: &str,
        target: &str,
        ttl_secs: Option<u64>,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            grant,
            tool: tool.to_string(),
            action: action.to_string(),
            target: target.to_string(),
            expires_at: ttl_secs.map(|ttl| now + ttl),
        }
    }

    /// Check if this entry has expired.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            None => false, // Permanent entries never expire
            Some(expiry) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now >= expiry
            }
        }
    }

    /// Returns true if this cache entry matches the given tool, action, and target.
    pub fn matches(&self, tool: &str, action: &str, target: &str) -> bool {
        self.tool == tool && self.action == action && self.target == target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_rule_creation() {
        let rule = PermissionRule::new("r1", "Test rule", "FileRead", "*");
        assert_eq!(rule.id, "r1");
        assert_eq!(rule.description, "Test rule");
        assert_eq!(rule.tool, "FileRead");
        assert_eq!(rule.action, "*");
        assert!(!rule.require_confirmation);
        assert!(!rule.auto_deny);
    }

    #[test]
    fn test_permission_rule_with_target() {
        let rule = PermissionRule::new("r2", "Deny env files", "FileWrite", "*")
            .with_target("*.env")
            .with_auto_deny();

        assert!(rule.auto_deny);
        assert_eq!(rule.target_pattern, Some("*.env".to_string()));
        assert!(rule.matches_target(".env"));
        assert!(rule.matches_target("config.env"));
        assert!(!rule.matches_target("main.rs"));
    }

    #[test]
    fn test_permission_rule_wildcard_target() {
        let rule = PermissionRule::new("r3", "Catch all", "Bash", "*");
        assert!(rule.matches_target("anything"));
        assert!(rule.matches_target("rm -rf /"));
    }

    #[test]
    fn test_permission_request_new() {
        let req = PermissionRequest::new("Bash", "git status", ".");
        assert_eq!(req.tool, "Bash");
        assert_eq!(req.action, "git status");
        assert_eq!(req.target, ".");
        assert!(!req.id.is_empty());
        assert!(req.timestamp > 0);
    }

    #[test]
    fn test_decision_is_allowed() {
        assert!(DecisionType::Allow.is_allowed());
        assert!(DecisionType::AllowOnce.is_allowed());
        assert!(DecisionType::AllowSession.is_allowed());
        assert!(DecisionType::AllowPermanent.is_allowed());
        assert!(!DecisionType::Deny.is_allowed());
        assert!(!DecisionType::AskUser.is_allowed());
    }

    #[test]
    fn test_decision_from_rule() {
        let allow_rule = PermissionRule::new("r-allow", "Allow rule", "FileRead", "*");
        let decision = PermissionDecision::from_rule("req-1", &allow_rule);
        assert_eq!(decision.decision, DecisionType::Allow);
        assert_eq!(decision.rule_applied, Some("r-allow".to_string()));

        let deny_rule = PermissionRule::new("r-deny", "Deny rule", "Bash", "rm")
            .with_auto_deny();
        let decision = PermissionDecision::from_rule("req-2", &deny_rule);
        assert_eq!(decision.decision, DecisionType::Deny);

        let ask_rule = PermissionRule::new("r-ask", "Ask rule", "FileWrite", "*.rs")
            .with_confirmation();
        let decision = PermissionDecision::from_rule("req-3", &ask_rule);
        assert_eq!(decision.decision, DecisionType::AskUser);
    }

    #[test]
    fn test_cache_entry_expiry() {
        // Permanent entry (no expiry)
        let perm = CacheEntry::new(
            DecisionType::AllowPermanent,
            "Bash",
            "git status",
            ".",
            None,
        );
        assert!(!perm.is_expired());

        // Temporary entry with future expiry
        let temp = CacheEntry::new(
            DecisionType::AllowSession,
            "FileWrite",
            "edit",
            "src/main.rs",
            Some(3600),
        );
        assert!(!temp.is_expired());

        // Entry that should be expired
        let expired = CacheEntry {
            grant: DecisionType::AllowOnce,
            tool: "Bash".to_string(),
            action: "ls".to_string(),
            target: "/tmp".to_string(),
            expires_at: Some(0), // Unix epoch
        };
        assert!(expired.is_expired());
    }

    #[test]
    fn test_cache_entry_matches() {
        let entry = CacheEntry::new(
            DecisionType::AllowSession,
            "Bash",
            "npm install",
            "node_modules",
            Some(300),
        );
        assert!(entry.matches("Bash", "npm install", "node_modules"));
        assert!(!entry.matches("Bash", "npm test", "node_modules"));
        assert!(!entry.matches("FileWrite", "npm install", "node_modules"));
        assert!(!entry.matches("Bash", "npm install", "src/"));
    }
}
