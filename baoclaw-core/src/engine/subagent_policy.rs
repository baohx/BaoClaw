//! Sub-agent execution policy — depth limits and tool isolation per nesting level.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Policy governing sub-agent execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubagentPolicy {
    /// Maximum nesting depth (0 = no sub-agents allowed, 1 = one level, etc.)
    pub max_depth: u32,
    /// Tool whitelist per depth level (depth 0 = root agent).
    pub depth_tool_whitelist: Vec<DepthToolSet>,
    /// Default policy for depths beyond explicit configuration.
    pub default_policy: DepthAction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepthToolSet {
    /// Depth level this applies to.
    pub depth: u32,
    /// Tools allowed at this depth.
    pub allowed_tools: HashSet<String>,
    /// Maximum turns at this depth.
    pub max_turns: u32,
    /// Maximum cost at this depth (USD).
    pub max_cost_usd: f64,
    /// Action when budget exceeded.
    pub budget_exceeded_action: DepthAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DepthAction {
    /// Terminate the sub-agent.
    Terminate,
    /// Escalate to parent agent.
    Escalate,
    /// Allow but log a warning.
    WarnAndContinue,
}

impl Default for SubagentPolicy {
    fn default() -> Self {
        Self {
            max_depth: 3,
            depth_tool_whitelist: vec![
                // Depth 0 (root agent): all tools
                DepthToolSet {
                    depth: 0,
                    allowed_tools: HashSet::new(), // empty = all allowed
                    max_turns: 100,
                    max_cost_usd: 10.0,
                    budget_exceeded_action: DepthAction::WarnAndContinue,
                },
                // Depth 1: safe tools only
                DepthToolSet {
                    depth: 1,
                    allowed_tools: [
                        "FileRead".into(),
                        "FileEdit".into(),
                        "FileWrite".into(),
                        "Bash".into(),
                        "Grep".into(),
                        "Glob".into(),
                    ]
                    .into_iter()
                    .collect(),
                    max_turns: 30,
                    max_cost_usd: 2.0,
                    budget_exceeded_action: DepthAction::Terminate,
                },
                // Depth 2: read-only tools
                DepthToolSet {
                    depth: 2,
                    allowed_tools: [
                        "FileRead".into(),
                        "Bash".into(),
                        "Grep".into(),
                        "Glob".into(),
                    ]
                    .into_iter()
                    .collect(),
                    max_turns: 15,
                    max_cost_usd: 0.5,
                    budget_exceeded_action: DepthAction::Terminate,
                },
                // Depth 3: minimal tools
                DepthToolSet {
                    depth: 3,
                    allowed_tools: ["FileRead".into(), "Bash".into()]
                        .into_iter()
                        .collect(),
                    max_turns: 5,
                    max_cost_usd: 0.1,
                    budget_exceeded_action: DepthAction::Terminate,
                },
            ],
            default_policy: DepthAction::Terminate,
        }
    }
}

impl SubagentPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a tool is allowed at the given depth.
    pub fn is_tool_allowed(&self, tool_name: &str, depth: u32) -> bool {
        if depth > self.max_depth {
            return false;
        }
        let tool_set = self
            .depth_tool_whitelist
            .iter()
            .find(|ds| ds.depth == depth);
        match tool_set {
            Some(ds) => {
                // Empty set = all tools allowed
                ds.allowed_tools.is_empty() || ds.allowed_tools.contains(tool_name)
            }
            None => false, // No config for this depth = denied
        }
    }

    /// Get the maximum turns allowed at the given depth.
    pub fn max_turns(&self, depth: u32) -> u32 {
        self.depth_tool_whitelist
            .iter()
            .find(|ds| ds.depth == depth)
            .map(|ds| ds.max_turns)
            .unwrap_or(5) // conservative default
    }

    /// Get the maximum cost allowed at the given depth.
    pub fn max_cost(&self, depth: u32) -> f64 {
        self.depth_tool_whitelist
            .iter()
            .find(|ds| ds.depth == depth)
            .map(|ds| ds.max_cost_usd)
            .unwrap_or(0.1)
    }

    /// Filter a list of tools to only those allowed at the given depth.
    pub fn filter_tools(&self, tools: &[String], depth: u32) -> Vec<String> {
        tools
            .iter()
            .filter(|t| self.is_tool_allowed(t, depth))
            .cloned()
            .collect()
    }

    /// Get a description of the policy for a given depth.
    pub fn describe(&self, depth: u32) -> String {
        if depth > self.max_depth {
            return format!("Depth {} exceeds maximum ({})", depth, self.max_depth);
        }
        let tool_set = self
            .depth_tool_whitelist
            .iter()
            .find(|ds| ds.depth == depth);
        match tool_set {
            Some(ds) => {
                let tools = if ds.allowed_tools.is_empty() {
                    "all tools".into()
                } else {
                    let mut t: Vec<&str> = ds.allowed_tools.iter().map(|s| s.as_str()).collect();
                    t.sort();
                    t.join(", ")
                };
                format!(
                    "Depth {}: {} tools, max {} turns, ${:.2} budget",
                    depth, tools, ds.max_turns, ds.max_cost_usd
                )
            }
            None => format!("Depth {}: no policy defined (blocked)", depth),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_depth_0_all_tools() {
        let policy = SubagentPolicy::new();
        assert!(policy.is_tool_allowed("Bash", 0));
        assert!(policy.is_tool_allowed("FileEdit", 0));
        assert!(policy.is_tool_allowed("DangerousTool", 0)); // all allowed
    }

    #[test]
    fn test_depth_1_safe_tools() {
        let policy = SubagentPolicy::new();
        assert!(policy.is_tool_allowed("FileRead", 1));
        assert!(policy.is_tool_allowed("Bash", 1));
        assert!(!policy.is_tool_allowed("WebSearch", 1)); // not in whitelist
    }

    #[test]
    fn test_depth_2_readonly() {
        let policy = SubagentPolicy::new();
        assert!(policy.is_tool_allowed("FileRead", 2));
        assert!(!policy.is_tool_allowed("FileEdit", 2));
        assert!(!policy.is_tool_allowed("FileWrite", 2));
    }

    #[test]
    fn test_depth_exceeds_max() {
        let policy = SubagentPolicy::new();
        assert!(!policy.is_tool_allowed("FileRead", 4)); // max_depth=3
    }

    #[test]
    fn test_filter_tools() {
        let policy = SubagentPolicy::new();
        let tools = vec!["FileRead".into(), "FileEdit".into(), "WebSearch".into()];
        let filtered = policy.filter_tools(&tools, 2);
        assert_eq!(filtered, vec!["FileRead"]);
    }

    #[test]
    fn test_max_turns() {
        let policy = SubagentPolicy::new();
        assert_eq!(policy.max_turns(0), 100);
        assert_eq!(policy.max_turns(1), 30);
        assert_eq!(policy.max_turns(3), 5);
    }
}
