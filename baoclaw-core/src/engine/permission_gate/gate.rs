//! PermissionGate — the core permission evaluation engine.
//!
//! Maintains a list of permission rules and a permission cache.
//! When a tool requests an action, the gate checks cache first,
//! then evaluates rules in priority order, and returns a decision.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::cache::PermissionCache;
use super::types::{DecisionType, PermissionDecision, PermissionRequest, PermissionRule};

/// The core permission gate that evaluates tool access requests.
///
/// Combines a rule-based policy engine with a session-aware cache.
/// Default rules are loaded on construction for common safety patterns.
pub struct PermissionGate {
    /// Ordered list of permission rules (first match wins).
    rules: Vec<PermissionRule>,
    /// Thread-safe cache of user-granted permissions.
    cache: PermissionCache,
}

impl PermissionGate {
    /// Create a new PermissionGate with sensible default rules.
    ///
    /// Default rules:
    /// - `FileRead` is always allowed (read-only is safe).
    /// - `FileWrite` requires confirmation for most files, auto-deny for `.env` and `.git/*`.
    /// - `Bash rm -rf` is auto-denied (dangerous).
    /// - `Bash sudo` and `chmod 777` are auto-denied.
    /// - `Bash git status/diff/log` are allowed (safe read-only commands).
    /// - `Bash ls/cat/echo/find/grep` are allowed (informational commands).
    /// - All other `Bash` commands require confirmation.
    pub fn new() -> Self {
        let mut gate = Self {
            rules: Vec::new(),
            cache: PermissionCache::new(),
        };
        gate.load_default_rules();
        gate
    }

    /// Load the default safety rules.
    fn load_default_rules(&mut self) {
        // ── FileRead: always allowed ──
        self.rules.push(
            PermissionRule::new("default-fileread-allow", "FileRead is always allowed", "FileRead", "*"),
        );

        // ── FileWrite: require confirmation, deny dangerous patterns ──
        self.rules.push(
            PermissionRule::new("default-filewrite-deny-env", "Deny writing to .env files", "FileWrite", "*")
                .with_target("*.env")
                .with_auto_deny(),
        );
        self.rules.push(
            PermissionRule::new("default-filewrite-deny-git", "Deny writing to .git directory", "FileWrite", "*")
                .with_target(".git/*")
                .with_auto_deny(),
        );
        self.rules.push(
            PermissionRule::new("default-filewrite-deny-ssh", "Deny writing to SSH keys", "FileWrite", "*")
                .with_target("*/.ssh/*")
                .with_auto_deny(),
        );
        self.rules.push(
            PermissionRule::new(
                "default-filewrite-allow-md",
                "Allow writing markdown files",
                "FileWrite",
                "*",
            )
            .with_target("*.md")
        );
        self.rules.push(
            PermissionRule::new(
                "default-filewrite-ask",
                "FileWrite requires confirmation by default",
                "FileWrite",
                "*",
            )
            .with_confirmation(),
        );

        // ── Bash: block dangerous commands, allow safe ones ──
        // Auto-deny dangerous shell commands
        let dangerous_commands = vec![
            ("default-bash-deny-rmrf", "rm -rf /"),
            ("default-bash-deny-rmrf-home", "rm -rf ~"),
            ("default-bash-deny-sudo", "sudo "),
            ("default-bash-deny-chmod", "chmod 777"),
            ("default-bash-deny-dev-null", "> /dev/sda"),
            ("default-bash-deny-fork-bomb", ":(){ :|:& };:"),
            ("default-bash-deny-mkfs", "mkfs."),
            ("default-bash-deny-dd", "dd if="),
        ];

        for (id, cmd) in dangerous_commands {
            self.rules.push(
                PermissionRule::new(id, &format!("Auto-deny dangerous command: {}", cmd), "Bash", cmd)
                    .with_auto_deny(),
            );
        }

        // Allow safe read-only commands
        let safe_commands = vec![
            "git status",
            "git diff",
            "git log",
            "git branch",
            "ls ",
            "cat ",
            "echo ",
            "find ",
            "grep ",
            "head ",
            "tail ",
            "wc ",
            "pwd",
        ];

        for cmd in safe_commands {
            let id = format!(
                "default-bash-allow-{}",
                cmd.trim().replace(' ', "-")
            );
            self.rules.push(
                PermissionRule::new(
                    &id,
                    &format!("Allow safe command: {}", cmd.trim()),
                    "Bash",
                    cmd,
                ),
            );
        }

        // Bash default: require confirmation
        self.rules.push(
            PermissionRule::new(
                "default-bash-ask",
                "Bash commands require confirmation by default",
                "Bash",
                "*",
            )
            .with_confirmation(),
        );

        // ── FileDelete / FileEdit: require confirmation ──
        self.rules.push(
            PermissionRule::new(
                "default-filedelete-ask",
                "FileDelete requires confirmation",
                "FileDelete",
                "*",
            )
            .with_confirmation(),
        );
        self.rules.push(
            PermissionRule::new(
                "default-fileedit-ask",
                "FileEdit requires confirmation",
                "FileEdit",
                "*",
            )
            .with_confirmation(),
        );

        // ── WebFetch: require confirmation, deny sensitive patterns ──
        self.rules.push(
            PermissionRule::new(
                "default-webfetch-deny-localhost",
                "Deny fetching localhost resources",
                "WebFetch",
                "*",
            )
            .with_target("localhost:*")
            .with_auto_deny(),
        );
        self.rules.push(
            PermissionRule::new(
                "default-webfetch-deny-internal",
                "Deny fetching internal/private IPs",
                "WebFetch",
                "*",
            )
            .with_target("127.*")
            .with_auto_deny(),
        );
        self.rules.push(
            PermissionRule::new(
                "default-webfetch-deny-internal2",
                "Deny fetching internal IPs (10.x)",
                "WebFetch",
                "*",
            )
            .with_target("10.*")
            .with_auto_deny(),
        );
        self.rules.push(
            PermissionRule::new(
                "default-webfetch-ask",
                "WebFetch requires confirmation by default",
                "WebFetch",
                "*",
            )
            .with_confirmation(),
        );

        // ── WebSearch: allowed ──
        self.rules.push(
            PermissionRule::new(
                "default-websearch-allow",
                "WebSearch is always allowed",
                "WebSearch",
                "*",
            ),
        );
    }

    /// Check a permission request against the cache and rules.
    ///
    /// The evaluation order is:
    /// 1. Check the cache for an existing grant (first priority).
    /// 2. Evaluate rules in order — first matching rule wins.
    /// 3. If no rule matches, default to asking the user.
    pub fn check(&self, tool: &str, action: &str, target: &str) -> PermissionDecision {
        let request_id = uuid::Uuid::new_v4().to_string();

        // 1. Check cache first
        if let Some(cached) = self.cache.check(tool, action, target) {
            return PermissionDecision {
                request_id,
                decision: cached,
                rule_applied: Some("cache".to_string()),
                expires_at: None,
            };
        }

        // 2. Evaluate rules in order
        for rule in &self.rules {
            if !rule.matches_tool_action(tool, action) {
                continue;
            }
            if !rule.matches_target(target) {
                continue;
            }

            // Rule matched — return the decision
            return PermissionDecision::from_rule(&request_id, rule);
        }

        // 3. No rule matched — default to asking the user
        PermissionDecision::ask_user(&request_id)
    }

    /// Check a permission request (takes a PermissionRequest struct).
    pub fn check_request(&self, request: &PermissionRequest) -> PermissionDecision {
        let mut decision = self.check(&request.tool, &request.action, &request.target);
        decision.request_id = request.id.clone();
        decision
    }

    /// Add a new permission rule. Rules are evaluated in insertion order
    /// (first match wins), so more specific rules should be added first.
    pub fn add_rule(&mut self, rule: PermissionRule) {
        // Check if a rule with the same ID already exists
        if let Some(existing) = self.rules.iter_mut().find(|r| r.id == rule.id) {
            *existing = rule;
        } else {
            self.rules.push(rule);
        }
    }

    /// Remove a permission rule by its ID. Returns true if a rule was removed.
    pub fn remove_rule(&mut self, id: &str) -> bool {
        let len_before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() < len_before
    }

    /// Grant a permission decision and cache it if appropriate.
    ///
    /// Decisions of type `AllowSession` and `AllowPermanent` are stored
    /// in the cache for future automatic approval.
    ///
    /// # Arguments
    /// * `request_id` - The ID of the request being granted (for logging).
    /// * `decision` - The type of grant (AllowOnce, AllowSession, AllowPermanent).
    /// * `duration_secs` - For session grants, optional TTL in seconds.
    pub fn grant(
        &mut self,
        tool: &str,
        action: &str,
        target: &str,
        decision: DecisionType,
        duration_secs: Option<u64>,
    ) {
        match decision {
            DecisionType::AllowSession => {
                let ttl = duration_secs.or(Some(3600 * 24)); // Default: 24 hours
                self.cache.store(tool, action, target, DecisionType::AllowSession, ttl);
            }
            DecisionType::AllowPermanent => {
                self.cache
                    .store(tool, action, target, DecisionType::AllowPermanent, None);
            }
            // AllowOnce and Deny are not cached
            _ => {}
        }
    }

    /// Revoke a cached grant for the given tool, action, and target.
    /// Returns the number of cache entries removed.
    pub fn revoke(&mut self, tool: &str, action: &str, target: &str) -> usize {
        self.cache.revoke(tool, action, target)
    }

    /// Return a reference to the list of permission rules.
    pub fn list_rules(&self) -> &[PermissionRule] {
        &self.rules
    }

    /// Get the number of rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Clear all cached grants.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Clean up expired cache entries.
    pub fn cleanup_cache(&self) {
        self.cache.cleanup();
    }

    /// Reset to default rules and clear cache.
    pub fn reset(&mut self) {
        self.rules.clear();
        self.cache.clear();
        self.load_default_rules();
    }
}

impl Default for PermissionGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fileread_always_allowed() {
        let gate = PermissionGate::new();
        let decision = gate.check("FileRead", "read", "src/main.rs");
        assert_eq!(decision.decision, DecisionType::Allow);
        assert!(decision.rule_applied.is_some());
        assert!(decision.rule_applied.unwrap().contains("fileread"));
    }

    #[test]
    fn test_filewrite_env_denied() {
        let gate = PermissionGate::new();
        let decision = gate.check("FileWrite", "write", ".env");
        assert_eq!(decision.decision, DecisionType::Deny);
    }

    #[test]
    fn test_filewrite_md_allowed() {
        let gate = PermissionGate::new();
        let decision = gate.check("FileWrite", "write", "README.md");
        // .md files are explicitly allowed by the default rule
        assert_eq!(decision.decision, DecisionType::Allow);
    }

    #[test]
    fn test_bash_rmrf_denied() {
        let gate = PermissionGate::new();
        let decision = gate.check("Bash", "rm -rf /", "/");
        assert_eq!(decision.decision, DecisionType::Deny);
    }

    #[test]
    fn test_bash_safe_command_allowed() {
        let gate = PermissionGate::new();
        let decision = gate.check("Bash", "git status", ".");
        assert_eq!(decision.decision, DecisionType::Allow);
    }

    #[test]
    fn test_bash_unknown_requires_confirmation() {
        let gate = PermissionGate::new();
        let decision = gate.check("Bash", "npm install some-package", ".");
        assert_eq!(decision.decision, DecisionType::AskUser);
    }

    #[test]
    fn test_filewrite_dot_git_denied() {
        let gate = PermissionGate::new();
        let decision = gate.check("FileWrite", "write", ".git/config");
        assert_eq!(decision.decision, DecisionType::Deny);
    }

    #[test]
    fn test_add_and_remove_rule() {
        let mut gate = PermissionGate::new();
        let initial_count = gate.rule_count();

        let rule = PermissionRule::new("test-rule", "Test rule", "MyTool", "do-thing");
        gate.add_rule(rule);
        assert_eq!(gate.rule_count(), initial_count + 1);

        // Test with the new rule
        let decision = gate.check("MyTool", "do-thing", "anything");
        assert_eq!(decision.decision, DecisionType::Allow);
        assert_eq!(decision.rule_applied, Some("test-rule".to_string()));

        // Remove it
        assert!(gate.remove_rule("test-rule"));
        assert_eq!(gate.rule_count(), initial_count);

        // Now it should fall through to AskUser (no matching rule)
        let decision = gate.check("MyTool", "do-thing", "anything");
        assert_eq!(decision.decision, DecisionType::AskUser);
    }

    #[test]
    fn test_grant_and_cache() {
        let mut gate = PermissionGate::new();

        // Initially, this bash command requires confirmation
        let decision = gate.check("Bash", "npm run build", ".");
        assert_eq!(decision.decision, DecisionType::AskUser);

        // Grant it for the session
        gate.grant(
            "Bash",
            "npm run build",
            ".",
            DecisionType::AllowSession,
            Some(3600),
        );

        // Now it should be allowed via cache
        let decision = gate.check("Bash", "npm run build", ".");
        assert_eq!(decision.decision, DecisionType::AllowSession);
        assert_eq!(decision.rule_applied, Some("cache".to_string()));
    }

    #[test]
    fn test_grant_permanent() {
        let mut gate = PermissionGate::new();

        gate.grant(
            "FileWrite",
            "write",
            "src/main.rs",
            DecisionType::AllowPermanent,
            None,
        );

        let decision = gate.check("FileWrite", "write", "src/main.rs");
        assert_eq!(decision.decision, DecisionType::AllowPermanent);
    }

    #[test]
    fn test_revoke_cache() {
        let mut gate = PermissionGate::new();

        gate.grant(
            "Bash",
            "deploy",
            "prod",
            DecisionType::AllowSession,
            Some(3600),
        );

        // Should be allowed
        let decision = gate.check("Bash", "deploy", "prod");
        assert!(decision.decision.is_allowed());

        // Revoke
        let removed = gate.revoke("Bash", "deploy", "prod");
        assert_eq!(removed, 1);

        // Now should require confirmation again
        let decision = gate.check("Bash", "deploy", "prod");
        assert_eq!(decision.decision, DecisionType::AskUser);
    }

    #[test]
    fn test_reset() {
        let mut gate = PermissionGate::new();
        let original_count = gate.rule_count();

        // Add a custom rule
        gate.add_rule(PermissionRule::new("custom", "Custom", "ToolX", "doY"));
        assert_eq!(gate.rule_count(), original_count + 1);

        // Grant something
        gate.grant("Bash", "custom-cmd", ".", DecisionType::AllowPermanent, None);

        // Reset
        gate.reset();

        // Rules should be back to default
        assert_eq!(gate.rule_count(), original_count);

        // Cache should be cleared
        let decision = gate.check("Bash", "custom-cmd", ".");
        assert_eq!(decision.decision, DecisionType::AskUser);
    }

    #[test]
    fn test_webfetch_localhost_denied() {
        let gate = PermissionGate::new();
        let decision = gate.check("WebFetch", "fetch", "localhost:8080");
        assert_eq!(decision.decision, DecisionType::Deny);
    }

    #[test]
    fn test_webfetch_external_ask() {
        let gate = PermissionGate::new();
        let decision = gate.check("WebFetch", "fetch", "https://example.com");
        assert_eq!(decision.decision, DecisionType::AskUser);
    }

    #[test]
    fn test_websearch_allowed() {
        let gate = PermissionGate::new();
        let decision = gate.check("WebSearch", "search", "rust lang");
        assert_eq!(decision.decision, DecisionType::Allow);
    }

    #[test]
    fn test_no_rule_defaults_to_ask() {
        let gate = PermissionGate::new();
        // A tool with no rules at all
        let decision = gate.check("UnknownTool", "some-action", "some-target");
        assert_eq!(decision.decision, DecisionType::AskUser);
    }
}
