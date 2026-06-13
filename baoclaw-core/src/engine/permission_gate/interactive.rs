//! Interactive permission prompter for user-facing confirmation.
//!
//! Formats permission requests into readable prompts and parses
//! user responses into decision types. Supports quick-action
//! shortcuts for common responses.

use super::types::{DecisionType, PermissionRequest};

/// Formats permission requests for interactive user confirmation
/// and parses user responses back into decision types.
pub struct InteractivePrompter;

impl InteractivePrompter {
    /// Format a permission request as a human-readable prompt for the user.
    ///
    /// The output includes:
    /// - The requesting tool name
    /// - The action being attempted
    /// - The target resource
    /// - Available response options
    pub fn prompt(request: &PermissionRequest) -> String {
        let mut lines = Vec::new();

        lines.push(String::new());
        lines.push("╔══════════════════════════════════════════╗".to_string());
        lines.push("║  ⚠️  权限请求 (Permission Request)       ║".to_string());
        lines.push("╠══════════════════════════════════════════╣".to_string());
        lines.push(format!(
            "║  工具 (Tool):  {:24} ║",
            truncate(&request.tool, 24)
        ));
        lines.push(format!(
            "║  操作 (Action): {:24} ║",
            truncate(&request.action, 24)
        ));
        lines.push(format!(
            "║  目标 (Target): {:24} ║",
            truncate(&request.target, 24)
        ));
        lines.push("╠══════════════════════════════════════════╣".to_string());
        lines.push("║  可用操作:                               ║".to_string());

        for action in Self::quick_actions(request) {
            lines.push(format!("║    {}", action));
        }

        lines.push("╚══════════════════════════════════════════╝".to_string());
        lines.push(String::new());

        lines.join("\n")
    }

    /// Format a minimal prompt (suitable for TUI/CLI inline display).
    pub fn prompt_minimal(request: &PermissionRequest) -> String {
        format!(
            "⚠️  {} wants to {} on {} — [y]es/[n]o/[o]nce/[s]ession/[a]lways: ",
            request.tool, request.action, request.target
        )
    }

    /// Parse a user's text response into a `DecisionType`.
    ///
    /// Supported responses:
    /// - `y`, `yes`, `allow` → `AllowOnce`
    /// - `n`, `no`, `deny` → `Deny`
    /// - `o`, `once` → `AllowOnce`
    /// - `s`, `session` → `AllowSession`
    /// - `a`, `always`, `permanent` → `AllowPermanent`
    ///
    /// Returns `None` if the response cannot be parsed.
    pub fn parse_response(response: &str) -> Option<DecisionType> {
        let trimmed = response.trim().to_lowercase();

        match trimmed.as_str() {
            "y" | "yes" | "allow" => Some(DecisionType::AllowOnce),
            "n" | "no" | "deny" | "d" => Some(DecisionType::Deny),
            "o" | "once" => Some(DecisionType::AllowOnce),
            "s" | "session" => Some(DecisionType::AllowSession),
            "a" | "always" | "permanent" | "p" => Some(DecisionType::AllowPermanent),
            _ => None,
        }
    }

    /// Generate a list of quick-action descriptions for display.
    ///
    /// Each string describes one available action and its shortcut.
    pub fn quick_actions(request: &PermissionRequest) -> Vec<String> {
        vec![
            "[y] 允许本次 (Allow this once)".to_string(),
            "[n] 拒绝 (Deny)".to_string(),
            "[o] 允许本次 (Allow once)".to_string(),
            "[s] 本次会话允许 (Allow this session)".to_string(),
            "[a] 永久允许 (Always allow)".to_string(),
        ]
    }

    /// Parse a response and return a friendly error message if parsing fails.
    pub fn parse_response_or_error(response: &str) -> Result<DecisionType, String> {
        Self::parse_response(response).ok_or_else(|| {
            format!(
                "无法识别的响应 '{}'。有效选项: y/yes, n/no, o/once, s/session, a/always",
                response
            )
        })
    }
}

/// Truncate a string to `max_len` characters, adding "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request() -> PermissionRequest {
        PermissionRequest::new("Bash", "npm install express", "node_modules")
    }

    #[test]
    fn test_prompt_formatting() {
        let request = make_request();
        let prompt = InteractivePrompter::prompt(&request);
        assert!(prompt.contains("权限请求"));
        assert!(prompt.contains("Bash"));
        assert!(prompt.contains("npm install express"));
        assert!(prompt.contains("node_modules"));
        assert!(prompt.contains("[y]"));
        assert!(prompt.contains("[n]"));
        assert!(prompt.contains("[o]"));
        assert!(prompt.contains("[s]"));
        assert!(prompt.contains("[a]"));
    }

    #[test]
    fn test_prompt_minimal() {
        let request = make_request();
        let prompt = InteractivePrompter::prompt_minimal(&request);
        assert!(prompt.contains("Bash"));
        assert!(prompt.contains("npm install express"));
        assert!(prompt.contains("node_modules"));
        assert!(prompt.contains("[y]es/[n]o/[o]nce/[s]ession/[a]lways"));
    }

    #[test]
    fn test_parse_response_yes() {
        assert_eq!(
            InteractivePrompter::parse_response("y"),
            Some(DecisionType::AllowOnce)
        );
        assert_eq!(
            InteractivePrompter::parse_response("yes"),
            Some(DecisionType::AllowOnce)
        );
        assert_eq!(
            InteractivePrompter::parse_response("YES"),
            Some(DecisionType::AllowOnce)
        );
        assert_eq!(
            InteractivePrompter::parse_response("allow"),
            Some(DecisionType::AllowOnce)
        );
    }

    #[test]
    fn test_parse_response_no() {
        assert_eq!(
            InteractivePrompter::parse_response("n"),
            Some(DecisionType::Deny)
        );
        assert_eq!(
            InteractivePrompter::parse_response("no"),
            Some(DecisionType::Deny)
        );
        assert_eq!(
            InteractivePrompter::parse_response("deny"),
            Some(DecisionType::Deny)
        );
        assert_eq!(
            InteractivePrompter::parse_response("d"),
            Some(DecisionType::Deny)
        );
    }

    #[test]
    fn test_parse_response_once() {
        assert_eq!(
            InteractivePrompter::parse_response("o"),
            Some(DecisionType::AllowOnce)
        );
        assert_eq!(
            InteractivePrompter::parse_response("once"),
            Some(DecisionType::AllowOnce)
        );
    }

    #[test]
    fn test_parse_response_session() {
        assert_eq!(
            InteractivePrompter::parse_response("s"),
            Some(DecisionType::AllowSession)
        );
        assert_eq!(
            InteractivePrompter::parse_response("session"),
            Some(DecisionType::AllowSession)
        );
    }

    #[test]
    fn test_parse_response_always() {
        assert_eq!(
            InteractivePrompter::parse_response("a"),
            Some(DecisionType::AllowPermanent)
        );
        assert_eq!(
            InteractivePrompter::parse_response("always"),
            Some(DecisionType::AllowPermanent)
        );
        assert_eq!(
            InteractivePrompter::parse_response("permanent"),
            Some(DecisionType::AllowPermanent)
        );
        assert_eq!(
            InteractivePrompter::parse_response("p"),
            Some(DecisionType::AllowPermanent)
        );
    }

    #[test]
    fn test_parse_response_invalid() {
        assert_eq!(InteractivePrompter::parse_response("maybe"), None);
        assert_eq!(InteractivePrompter::parse_response(""), None);
        assert_eq!(InteractivePrompter::parse_response("xyz"), None);
    }

    #[test]
    fn test_parse_response_or_error() {
        assert!(InteractivePrompter::parse_response_or_error("y").is_ok());
        assert!(InteractivePrompter::parse_response_or_error("bad").is_err());
        let err = InteractivePrompter::parse_response_or_error("bad").unwrap_err();
        assert!(err.contains("无法识别的响应"));
    }

    #[test]
    fn test_quick_actions() {
        let request = make_request();
        let actions = InteractivePrompter::quick_actions(&request);
        assert_eq!(actions.len(), 5);
        assert!(actions.iter().any(|a| a.contains("[y]")));
        assert!(actions.iter().any(|a| a.contains("[n]")));
        assert!(actions.iter().any(|a| a.contains("[o]")));
        assert!(actions.iter().any(|a| a.contains("[s]")));
        assert!(actions.iter().any(|a| a.contains("[a]")));
    }

    #[test]
    fn test_truncate_long_content() {
        let long_action = "npm install really-long-package-name-that-exceeds-24-chars";
        let request = PermissionRequest::new("Bash", long_action, "/some/very/long/path");
        let prompt = InteractivePrompter::prompt(&request);
        // Should not panic and should contain truncated content
        assert!(prompt.contains("Bash"));
    }
}
