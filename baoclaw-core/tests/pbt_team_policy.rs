//! Property-based tests for team policy and sub-agent execution.
//!
//! These tests validate:
//! - Tool permission inheritance and restriction
//! - Budget control and enforcement
//! - Result collection correctness
//!
//! **Validates: Requirements FR-2.4 资源限制**
//! - Each Agent's tool permission inheritance
//! - Total cost/Token budget

use proptest::prelude::*;
use std::collections::HashSet;

// Import from the crate
use baoclaw_core::engine::team::agent::BudgetEnforcer;
use baoclaw_core::engine::team::policy::{AgentPolicy, BudgetExceededAction, DepthTools};
use baoclaw_core::engine::team::TeamPolicy;

/// Strategy for generating valid tool names
fn tool_name_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("FileRead".to_string()),
        Just("FileWrite".to_string()),
        Just("FileEdit".to_string()),
        Just("Bash".to_string()),
        Just("Grep".to_string()),
        Just("Glob".to_string()),
        Just("WebSearch".to_string()),
        Just("WebFetch".to_string()),
    ]
}

/// Strategy for generating a set of tool names
fn tool_set_strategy() -> impl Strategy<Value = HashSet<String>> {
    proptest::collection::hash_set(tool_name_strategy(), 0..8)
}

/// Strategy for generating depth values
fn depth_strategy() -> impl Strategy<Value = u32> {
    0u32..5
}

/// Strategy for generating cost values
fn cost_strategy() -> impl Strategy<Value = f64> {
    0.0f64..100.0
}

/// Strategy for generating token counts
fn tokens_strategy() -> impl Strategy<Value = u64> {
    0u64..1000000
}

proptest! {
    /// Test that tool whitelist filtering is consistent
    ///
    /// **Validates: FR-2.4 工具权限继承**
    /// - Tools in whitelist are allowed
    /// - Tools not in whitelist are denied
    #[test]
    fn prop_tool_whitelist_consistency(
        whitelist in tool_set_strategy(),
        tool in tool_name_strategy(),
        depth in depth_strategy()
    ) {
        let policy = TeamPolicy::default()
            .with_tool_whitelist(whitelist.iter().cloned().collect())
            .with_max_depth(10); // Allow deeper nesting

        let is_allowed = policy.is_tool_allowed(&tool, depth);
        let is_in_whitelist = whitelist.contains(&tool);

        // If whitelist is empty, all tools are allowed
        if whitelist.is_empty() {
            prop_assert!(is_allowed);
        } else {
            // Otherwise, only whitelisted tools are allowed
            prop_assert_eq!(is_allowed, is_in_whitelist);
        }
    }

    /// Test that tool blacklist filtering is consistent
    ///
    /// **Validates: FR-2.4 工具权限限制**
    /// - Tools in blacklist are always denied
    #[test]
    fn prop_tool_blacklist_consistency(
        blacklist in tool_set_strategy(),
        tool in tool_name_strategy(),
        depth in depth_strategy()
    ) {
        let policy = TeamPolicy::default()
            .with_tool_blacklist(blacklist.iter().cloned().collect())
            .with_max_depth(10); // Allow deeper nesting

        let is_allowed = policy.is_tool_allowed(&tool, depth);
        let is_in_blacklist = blacklist.contains(&tool);

        // Blacklisted tools are always denied
        if is_in_blacklist {
            prop_assert!(!is_allowed);
        } else {
            // Non-blacklisted tools are allowed (when no whitelist)
            prop_assert!(is_allowed);
        }
    }

    /// Test that blacklist takes precedence over whitelist
    ///
    /// **Validates: FR-2.4 工具权限继承与限制**
    #[test]
    fn prop_blacklist_precedence(
        whitelist in tool_set_strategy(),
        blacklist in tool_set_strategy(),
        tool in tool_name_strategy(),
        depth in depth_strategy()
    ) {
        let policy = TeamPolicy::default()
            .with_tool_whitelist(whitelist.iter().cloned().collect())
            .with_tool_blacklist(blacklist.iter().cloned().collect())
            .with_max_depth(10); // Allow deeper nesting

        let is_allowed = policy.is_tool_allowed(&tool, depth);
        let is_in_blacklist = blacklist.contains(&tool);

        // Blacklisted tools are always denied, even if in whitelist
        if is_in_blacklist {
            prop_assert!(!is_allowed);
        }
    }

    /// Test max depth restriction
    ///
    /// **Validates: FR-2.4 资源限制 - 最大并行数限制**
    #[test]
    fn prop_max_depth_restriction(
        max_depth in depth_strategy(),
        query_depth in depth_strategy()
    ) {
        let policy = TeamPolicy::default().with_max_depth(max_depth);

        let is_allowed = policy.is_depth_allowed(query_depth);

        // Depth should only be allowed if within max_depth
        prop_assert_eq!(is_allowed, query_depth <= max_depth);
    }

    /// Test budget exceeded detection
    ///
    /// **Validates: FR-2.4 资源限制 - 总成本/Token 预算**
    #[test]
    fn prop_budget_exceeded_detection(
        max_cost in cost_strategy(),
        current_cost in cost_strategy()
    ) {
        // Avoid division by zero
        let max_cost = max_cost.max(0.01);

        let policy = TeamPolicy::default().with_total_budget(max_cost);

        let is_exceeded = policy.is_budget_exceeded(current_cost, 0);

        // Budget is exceeded when current >= max
        prop_assert_eq!(is_exceeded, current_cost >= max_cost);
    }

    /// Test token budget exceeded detection
    ///
    /// **Validates: FR-2.4 资源限制 - Token 预算**
    #[test]
    fn prop_token_budget_exceeded_detection(
        max_tokens in tokens_strategy(),
        current_tokens in tokens_strategy()
    ) {
        let mut policy = TeamPolicy::default();
        policy.total_budget_tokens = Some(max_tokens);

        let is_exceeded = policy.is_budget_exceeded(0.0, current_tokens);

        // Token budget is exceeded when current >= max
        prop_assert_eq!(is_exceeded, current_tokens >= max_tokens);
    }

    /// Test agent policy depth inheritance
    ///
    /// **Validates: FR-2.4 Agent 的工具权限继承**
    #[test]
    fn prop_agent_policy_depth_inheritance(
        depth in depth_strategy()
    ) {
        let team_policy = TeamPolicy::default()
            .with_max_turns_per_agent(10)
            .with_max_cost_per_agent(1.0);

        let agent_policy = AgentPolicy::from_team_policy(&team_policy, depth);

        // Agent policy should inherit from team policy
        prop_assert_eq!(agent_policy.depth, depth);
        prop_assert_eq!(agent_policy.max_turns, team_policy.max_turns_per_agent);
        prop_assert_eq!(agent_policy.max_cost_usd, team_policy.max_cost_per_agent);
    }

    /// Test policy description contains key information
    #[test]
    fn prop_policy_description_informative(
        max_cost in cost_strategy(),
        max_turns in 1u32..20u32
    ) {
        let policy = TeamPolicy::default()
            .with_total_budget(max_cost)
            .with_max_turns_per_agent(max_turns);

        let description = policy.describe();

        // Description should contain key policy parameters
        prop_assert!(description.contains("max_depth"));
        let turns_str = format!("max_turns={}", max_turns);
        prop_assert!(description.contains(&turns_str));
    }

    /// Test depth-specific tool restrictions
    ///
    /// **Validates: FR-2.4 每个 Agent 的工具权限继承**
    #[test]
    fn prop_depth_tool_restrictions(
        depth in depth_strategy(),
        allowed_tools in tool_set_strategy(),
        denied_tools in tool_set_strategy()
    ) {
        let mut policy = TeamPolicy::default();

        // Only add restriction for valid depth
        if depth <= 3 {
            policy.depth_tool_restrictions.insert(
                depth,
                DepthTools {
                    depth,
                    allowed_tools: allowed_tools.clone(),
                    denied_tools: denied_tools.clone(),
                    max_turns: None,
                    max_cost_usd: None,
                },
            );

            // Check that depth-specific restrictions are applied
            for tool in &allowed_tools {
                if !denied_tools.contains(tool) {
                    prop_assert!(policy.is_tool_allowed(tool, depth));
                }
            }

            for tool in &denied_tools {
                prop_assert!(!policy.is_tool_allowed(tool, depth));
            }
        }
    }

    /// Test budget enforcer incremental tracking
    ///
    /// **Validates: FR-2.4 预算控制**
    #[test]
    fn prop_budget_enforcer_incremental(
        initial_max_cost in cost_strategy(),
        cost_updates in proptest::collection::vec(cost_strategy(), 1..10)
    ) {
        let max_cost = initial_max_cost.max(0.01);

        let team_policy = TeamPolicy::default()
            .with_max_cost_per_agent(max_cost);

        let agent_policy = AgentPolicy::from_team_policy(&team_policy, 1);
        let mut enforcer = baoclaw_core::engine::team::agent::BudgetEnforcer::from_policy(&agent_policy);

        let mut total_cost = 0.0_f64;
        let mut all_within_budget = true;

        for cost in cost_updates {
            let within_budget = enforcer.update_cost(cost);
            total_cost += cost;

            if total_cost >= max_cost {
                all_within_budget = false;
            }

            // Enforcer should detect budget exceeded at the right time
            if !within_budget {
                prop_assert!(enforcer.is_exceeded);
                prop_assert!(!all_within_budget || total_cost >= max_cost);
            }
        }
    }

    /// Test max turns enforcement
    ///
    /// **Validates: FR-2.4 每个 Agent 的工具权限继承 - 最大轮次限制**
    #[test]
    fn prop_max_turns_enforcement(
        max_turns in 1u32..10u32,
        actual_turns in 0u32..15u32
    ) {
        let team_policy = TeamPolicy::default()
            .with_max_turns_per_agent(max_turns);

        let agent_policy = AgentPolicy::from_team_policy(&team_policy, 1);
        let mut enforcer = baoclaw_core::engine::team::agent::BudgetEnforcer::from_policy(&agent_policy);

        let mut all_within_limit = true;

        for _ in 0..actual_turns {
            let within_limit = enforcer.increment_turn();
            if !within_limit {
                all_within_limit = false;
            }
        }

        // Check consistency
        if actual_turns <= max_turns {
            prop_assert!(all_within_limit || enforcer.is_exceeded);
        } else {
            // Once exceeded, stays exceeded
            prop_assert!(!all_within_limit);
        }
    }
}
