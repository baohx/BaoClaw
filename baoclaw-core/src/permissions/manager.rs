use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PermissionMode {
    Default,
    Plan,
    BypassPermissions,
    Auto,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionRule {
    pub tool_name: String,
    pub rule_content: Option<String>,
}

pub type ToolPermissionRulesBySource = HashMap<String, Vec<PermissionRule>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolPermissionContext {
    pub mode: PermissionMode,
    pub additional_working_directories: HashMap<String, String>,
    pub always_allow_rules: ToolPermissionRulesBySource,
    pub always_deny_rules: ToolPermissionRulesBySource,
    pub always_ask_rules: ToolPermissionRulesBySource,
    pub is_bypass_permissions_mode_available: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PermissionResult {
    Allow,
    Ask { message: String },
    Deny { message: String },
}

pub struct PermissionManager {
    context: RwLock<ToolPermissionContext>,
}

/// Simple glob matching with `*` wildcard.
/// `*` matches any sequence of characters (including empty).
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();
    let p_len = pattern_bytes.len();
    let t_len = text_bytes.len();

    // DP approach: dp[i][j] = pattern[0..i] matches text[0..j]
    // Use two rows for space efficiency
    let mut prev = vec![false; t_len + 1];
    prev[0] = true;

    // Handle leading *s
    for i in 0..p_len {
        if pattern_bytes[i] == b'*' {
            prev[0] = true;
        } else {
            break;
        }
    }

    // Fill first row: pattern[0..i] vs empty text is only true if all *
    // Already handled above for prev[0]; for j>0 with empty pattern it's false.

    // Actually, let's redo with a cleaner DP
    // dp[i][j] means pattern[0..i] matches text[0..j]
    let mut dp = vec![vec![false; t_len + 1]; p_len + 1];
    dp[0][0] = true;

    for i in 1..=p_len {
        if pattern_bytes[i - 1] == b'*' {
            dp[i][0] = dp[i - 1][0];
        }
    }

    for i in 1..=p_len {
        for j in 1..=t_len {
            if pattern_bytes[i - 1] == b'*' {
                // * matches zero chars (dp[i-1][j]) or one more char (dp[i][j-1])
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if pattern_bytes[i - 1].to_ascii_lowercase()
                == text_bytes[j - 1].to_ascii_lowercase()
            {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }

    dp[p_len][t_len]
}

/// Check if a permission rule matches the given tool name and input description.
fn matches_rule(
    rule: &PermissionRule,
    tool_name: &str,
    input_description: Option<&str>,
) -> bool {
    // Tool name must match (case-insensitive)
    if !rule.tool_name.eq_ignore_ascii_case(tool_name) {
        return false;
    }

    // If rule has content, input_description must match the glob pattern
    match (&rule.rule_content, input_description) {
        (Some(pattern), Some(desc)) => glob_matches(pattern, desc),
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn is_read_only_tool(tool_name: &str) -> bool {
    let read_only_patterns = ["Read", "Grep", "Glob", "Search"];
    read_only_patterns
        .iter()
        .any(|p| tool_name.contains(p))
}

fn find_matching_rule_in_map(
    rules: &ToolPermissionRulesBySource,
    tool_name: &str,
    input_description: Option<&str>,
) -> bool {
    rules.values().any(|rule_list| {
        rule_list
            .iter()
            .any(|rule| matches_rule(rule, tool_name, input_description))
    })
}

impl PermissionManager {
    pub fn new(context: ToolPermissionContext) -> Self {
        Self {
            context: RwLock::new(context),
        }
    }

    /// Check permission for a tool invocation.
    ///
    /// Evaluation order:
    /// 1. Deny rules (highest priority)
    /// 2. Allow rules
    /// 3. Ask rules
    /// 4. Mode-specific defaults
    /// 5. Default: Ask
    pub fn check_permission(
        &self,
        tool_name: &str,
        input_description: Option<&str>,
    ) -> PermissionResult {
        let ctx = self.context.read().unwrap();

        // Step 1: Check deny rules first (highest priority)
        if find_matching_rule_in_map(&ctx.always_deny_rules, tool_name, input_description) {
            return PermissionResult::Deny {
                message: format!("Tool '{}' is denied by permission rules", tool_name),
            };
        }

        // Step 2: In BypassPermissions mode, allow all non-denied tools
        if ctx.mode == PermissionMode::BypassPermissions {
            return PermissionResult::Allow;
        }

        // Step 3: Check allow rules
        if find_matching_rule_in_map(&ctx.always_allow_rules, tool_name, input_description) {
            return PermissionResult::Allow;
        }

        // Step 4: Check ask rules
        if find_matching_rule_in_map(&ctx.always_ask_rules, tool_name, input_description) {
            return PermissionResult::Ask {
                message: format!(
                    "Tool '{}' requires permission (matched ask rule)",
                    tool_name
                ),
            };
        }

        // Step 5: Plan mode - allow read-only tools, ask for others
        if ctx.mode == PermissionMode::Plan {
            if is_read_only_tool(tool_name) {
                return PermissionResult::Allow;
            }
            return PermissionResult::Ask {
                message: format!(
                    "Tool '{}' requires permission in Plan mode",
                    tool_name
                ),
            };
        }

        // Step 6: Default - Ask
        PermissionResult::Ask {
            message: format!("Tool '{}' requires permission", tool_name),
        }
    }

    /// Update the permission context using a closure.
    pub fn update_context(&self, updater: impl FnOnce(&mut ToolPermissionContext)) {
        let mut ctx = self.context.write().unwrap();
        updater(&mut ctx);
    }

    /// Get a clone of the current permission context.
    pub fn get_context(&self) -> ToolPermissionContext {
        self.context.read().unwrap().clone()
    }

    /// Add an "always allow" rule for a specific tool from a given source.
    pub fn add_allow_always_rule(
        &self,
        source: &str,
        tool_name: &str,
        rule_content: Option<String>,
    ) {
        let mut ctx = self.context.write().unwrap();
        ctx.always_allow_rules
            .entry(source.to_string())
            .or_default()
            .push(PermissionRule {
                tool_name: tool_name.to_string(),
                rule_content,
            });
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn empty_context() -> ToolPermissionContext {
        ToolPermissionContext {
            mode: PermissionMode::Default,
            additional_working_directories: HashMap::new(),
            always_allow_rules: HashMap::new(),
            always_deny_rules: HashMap::new(),
            always_ask_rules: HashMap::new(),
            is_bypass_permissions_mode_available: false,
        }
    }

    // --- glob_matches tests ---

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_matches("hello", "hello"));
        assert!(!glob_matches("hello", "world"));
    }

    #[test]
    fn test_glob_star_matches_any() {
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("*", ""));
        assert!(glob_matches("git *", "git push origin main"));
        assert!(glob_matches("git *", "git status"));
        assert!(!glob_matches("git *", "npm install"));
    }

    #[test]
    fn test_glob_star_in_middle() {
        assert!(glob_matches("a*c", "abc"));
        assert!(glob_matches("a*c", "aXYZc"));
        assert!(!glob_matches("a*c", "aXYZd"));
    }

    #[test]
    fn test_glob_multiple_stars() {
        assert!(glob_matches("a*b*c", "aXbYc"));
        assert!(glob_matches("a*b*c", "abc"));
        assert!(glob_matches("*a*", "xax"));
        assert!(glob_matches("*a*", "a"));
    }

    #[test]
    fn test_glob_case_insensitive() {
        assert!(glob_matches("Git", "git"));
        assert!(glob_matches("GIT *", "git push"));
    }

    // --- matches_rule tests ---

    #[test]
    fn test_rule_matches_tool_name_only() {
        let rule = PermissionRule {
            tool_name: "Bash".to_string(),
            rule_content: None,
        };
        assert!(matches_rule(&rule, "Bash", None));
        assert!(matches_rule(&rule, "bash", None));
        assert!(matches_rule(&rule, "Bash", Some("anything")));
        assert!(!matches_rule(&rule, "FileRead", None));
    }

    #[test]
    fn test_rule_matches_with_content_pattern() {
        let rule = PermissionRule {
            tool_name: "Bash".to_string(),
            rule_content: Some("git *".to_string()),
        };
        assert!(matches_rule(&rule, "Bash", Some("git push origin main")));
        assert!(matches_rule(&rule, "Bash", Some("git status")));
        assert!(!matches_rule(&rule, "Bash", Some("npm install")));
        assert!(!matches_rule(&rule, "Bash", None));
    }

    // --- PermissionManager tests ---

    #[test]
    fn test_deny_rule_priority_over_allow() {
        let mut ctx = empty_context();
        ctx.always_allow_rules.insert(
            "user".to_string(),
            vec![PermissionRule {
                tool_name: "Bash".to_string(),
                rule_content: None,
            }],
        );
        ctx.always_deny_rules.insert(
            "system".to_string(),
            vec![PermissionRule {
                tool_name: "Bash".to_string(),
                rule_content: None,
            }],
        );

        let manager = PermissionManager::new(ctx);
        let result = manager.check_permission("Bash", None);
        assert!(matches!(result, PermissionResult::Deny { .. }));
    }

    #[test]
    fn test_allow_rule_returns_allow() {
        let mut ctx = empty_context();
        ctx.always_allow_rules.insert(
            "user".to_string(),
            vec![PermissionRule {
                tool_name: "Bash".to_string(),
                rule_content: None,
            }],
        );

        let manager = PermissionManager::new(ctx);
        assert_eq!(
            manager.check_permission("Bash", None),
            PermissionResult::Allow
        );
    }

    #[test]
    fn test_no_rules_returns_ask() {
        let ctx = empty_context();
        let manager = PermissionManager::new(ctx);
        let result = manager.check_permission("Bash", None);
        assert!(matches!(result, PermissionResult::Ask { .. }));
    }

    #[test]
    fn test_bypass_permissions_mode() {
        let mut ctx = empty_context();
        ctx.mode = PermissionMode::BypassPermissions;

        let manager = PermissionManager::new(ctx);
        assert_eq!(
            manager.check_permission("Bash", None),
            PermissionResult::Allow
        );
        assert_eq!(
            manager.check_permission("FileWrite", None),
            PermissionResult::Allow
        );
    }

    #[test]
    fn test_bypass_mode_still_denies() {
        let mut ctx = empty_context();
        ctx.mode = PermissionMode::BypassPermissions;
        ctx.always_deny_rules.insert(
            "system".to_string(),
            vec![PermissionRule {
                tool_name: "DangerousTool".to_string(),
                rule_content: None,
            }],
        );

        let manager = PermissionManager::new(ctx);
        assert!(matches!(
            manager.check_permission("DangerousTool", None),
            PermissionResult::Deny { .. }
        ));
        // Other tools still allowed
        assert_eq!(
            manager.check_permission("Bash", None),
            PermissionResult::Allow
        );
    }

    #[test]
    fn test_plan_mode_read_only_tools() {
        let mut ctx = empty_context();
        ctx.mode = PermissionMode::Plan;

        let manager = PermissionManager::new(ctx);

        // Read-only tools should be allowed
        assert_eq!(
            manager.check_permission("FileRead", None),
            PermissionResult::Allow
        );
        assert_eq!(
            manager.check_permission("GrepTool", None),
            PermissionResult::Allow
        );
        assert_eq!(
            manager.check_permission("GlobTool", None),
            PermissionResult::Allow
        );
        assert_eq!(
            manager.check_permission("WebSearch", None),
            PermissionResult::Allow
        );

        // Write tools should ask
        let result = manager.check_permission("Bash", None);
        assert!(matches!(result, PermissionResult::Ask { .. }));
        let result = manager.check_permission("FileWrite", None);
        assert!(matches!(result, PermissionResult::Ask { .. }));
    }

    #[test]
    fn test_wildcard_matching_in_rules() {
        let mut ctx = empty_context();
        ctx.always_allow_rules.insert(
            "user".to_string(),
            vec![PermissionRule {
                tool_name: "Bash".to_string(),
                rule_content: Some("git *".to_string()),
            }],
        );

        let manager = PermissionManager::new(ctx);

        // Matches the wildcard pattern
        assert_eq!(
            manager.check_permission("Bash", Some("git push origin main")),
            PermissionResult::Allow
        );
        assert_eq!(
            manager.check_permission("Bash", Some("git status")),
            PermissionResult::Allow
        );

        // Does not match the pattern
        let result = manager.check_permission("Bash", Some("npm install"));
        assert!(matches!(result, PermissionResult::Ask { .. }));

        // No input description, rule has content -> no match
        let result = manager.check_permission("Bash", None);
        assert!(matches!(result, PermissionResult::Ask { .. }));
    }

    #[test]
    fn test_deterministic_results() {
        let mut ctx = empty_context();
        ctx.always_allow_rules.insert(
            "user".to_string(),
            vec![PermissionRule {
                tool_name: "Bash".to_string(),
                rule_content: Some("git *".to_string()),
            }],
        );

        let manager = PermissionManager::new(ctx);

        // Same input should always produce the same result
        for _ in 0..10 {
            assert_eq!(
                manager.check_permission("Bash", Some("git push")),
                PermissionResult::Allow
            );
            assert!(matches!(
                manager.check_permission("Bash", Some("npm install")),
                PermissionResult::Ask { .. }
            ));
        }
    }

    #[test]
    fn test_update_context() {
        let ctx = empty_context();
        let manager = PermissionManager::new(ctx);

        // Initially no allow rules, should ask
        assert!(matches!(
            manager.check_permission("Bash", None),
            PermissionResult::Ask { .. }
        ));

        // Update context to add allow rule
        manager.update_context(|ctx| {
            ctx.always_allow_rules.insert(
                "user".to_string(),
                vec![PermissionRule {
                    tool_name: "Bash".to_string(),
                    rule_content: None,
                }],
            );
        });

        // Now should allow
        assert_eq!(
            manager.check_permission("Bash", None),
            PermissionResult::Allow
        );
    }

    #[test]
    fn test_add_allow_always_rule() {
        let ctx = empty_context();
        let manager = PermissionManager::new(ctx);

        // Initially should ask
        assert!(matches!(
            manager.check_permission("Bash", None),
            PermissionResult::Ask { .. }
        ));

        // Add allow always rule
        manager.add_allow_always_rule("user", "Bash", None);

        // Now should allow
        assert_eq!(
            manager.check_permission("Bash", None),
            PermissionResult::Allow
        );

        // Verify context was updated
        let updated_ctx = manager.get_context();
        assert_eq!(updated_ctx.always_allow_rules["user"].len(), 1);
        assert_eq!(updated_ctx.always_allow_rules["user"][0].tool_name, "Bash");
    }

    #[test]
    fn test_add_allow_always_rule_with_content() {
        let ctx = empty_context();
        let manager = PermissionManager::new(ctx);

        manager.add_allow_always_rule("user", "Bash", Some("git *".to_string()));

        assert_eq!(
            manager.check_permission("Bash", Some("git push")),
            PermissionResult::Allow
        );
        // Without matching content, still asks
        assert!(matches!(
            manager.check_permission("Bash", Some("npm install")),
            PermissionResult::Ask { .. }
        ));
    }

    #[test]
    fn test_multiple_sources() {
        let mut ctx = empty_context();
        ctx.always_allow_rules.insert(
            "user".to_string(),
            vec![PermissionRule {
                tool_name: "Bash".to_string(),
                rule_content: Some("git *".to_string()),
            }],
        );
        ctx.always_allow_rules.insert(
            "config".to_string(),
            vec![PermissionRule {
                tool_name: "Bash".to_string(),
                rule_content: Some("npm *".to_string()),
            }],
        );

        let manager = PermissionManager::new(ctx);

        assert_eq!(
            manager.check_permission("Bash", Some("git push")),
            PermissionResult::Allow
        );
        assert_eq!(
            manager.check_permission("Bash", Some("npm install")),
            PermissionResult::Allow
        );
        assert!(matches!(
            manager.check_permission("Bash", Some("rm -rf /")),
            PermissionResult::Ask { .. }
        ));
    }

    #[test]
    fn test_ask_rules() {
        let mut ctx = empty_context();
        ctx.always_ask_rules.insert(
            "system".to_string(),
            vec![PermissionRule {
                tool_name: "Bash".to_string(),
                rule_content: None,
            }],
        );

        let manager = PermissionManager::new(ctx);
        let result = manager.check_permission("Bash", None);
        assert!(matches!(result, PermissionResult::Ask { .. }));
    }

    #[test]
    fn test_get_context_returns_clone() {
        let mut ctx = empty_context();
        ctx.mode = PermissionMode::Plan;

        let manager = PermissionManager::new(ctx);
        let retrieved = manager.get_context();
        assert_eq!(retrieved.mode, PermissionMode::Plan);
    }
}
