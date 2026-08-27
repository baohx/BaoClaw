//! Team execution policy — tool permissions, budget control, and result collection.
//!
//! This module provides fine-grained control over sub-agent execution:
//! - Tool permission inheritance and restriction
//! - Per-agent and total team budget limits
//! - Structured result collection with metadata
//!
//! # Example
//!
//! ```rust,ignore
//! use baoclaw_core::engine::team::policy::{TeamPolicy, AgentPolicy};
//!
//! let policy = TeamPolicy::default()
//!     .with_tool_whitelist(vec!["FileRead", "Grep", "Glob"])
//!     .with_max_cost_per_agent(0.5)
//!     .with_total_budget(2.0);
//!
//! // Check if a tool is allowed for a sub-agent
//! assert!(policy.is_tool_allowed("FileRead", 1));
//! assert!(!policy.is_tool_allowed("WebSearch", 1));
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Policy governing team execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamPolicy {
    /// Maximum nesting depth for sub-agents (0 = no sub-agents allowed).
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,

    /// Tools that sub-agents are allowed to use.
    /// Empty means all tools are allowed (inherit from parent).
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub tool_whitelist: HashSet<String>,

    /// Tools that sub-agents are explicitly denied.
    /// Takes precedence over whitelist.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub tool_blacklist: HashSet<String>,

    /// Maximum cost per sub-agent in USD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_per_agent: Option<f64>,

    /// Maximum tokens per sub-agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_per_agent: Option<u64>,

    /// Maximum turns per sub-agent.
    #[serde(default = "default_max_turns")]
    pub max_turns_per_agent: u32,

    /// Maximum total cost for the entire team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_budget_usd: Option<f64>,

    /// Maximum total tokens for the entire team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_budget_tokens: Option<u64>,

    /// Maximum execution time per agent in seconds.
    #[serde(default = "default_timeout_secs")]
    pub agent_timeout_secs: u64,

    /// Maximum execution time for the entire team in seconds.
    #[serde(default = "default_team_timeout_secs")]
    pub team_timeout_secs: u64,

    /// Action to take when budget is exceeded.
    #[serde(default)]
    pub budget_exceeded_action: BudgetExceededAction,

    /// Per-depth tool restrictions.
    /// Allows different tool sets at different nesting levels.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub depth_tool_restrictions: HashMap<u32, DepthTools>,

    /// Whether sub-agents can spawn their own sub-agents.
    #[serde(default)]
    pub allow_nested_teams: bool,

    /// Whether to inherit tool permissions from parent agent.
    #[serde(default = "default_true")]
    pub inherit_parent_permissions: bool,
}

fn default_max_depth() -> u32 {
    3
}

fn default_max_turns() -> u32 {
    10
}

fn default_timeout_secs() -> u64 {
    300 // 5 minutes
}

fn default_team_timeout_secs() -> u64 {
    600 // 10 minutes
}

fn default_true() -> bool {
    true
}

/// Tool restrictions at a specific nesting depth.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepthTools {
    /// Depth level this applies to.
    pub depth: u32,

    /// Tools allowed at this depth.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub allowed_tools: HashSet<String>,

    /// Tools denied at this depth.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub denied_tools: HashSet<String>,

    /// Maximum turns at this depth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,

    /// Maximum cost at this depth in USD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

/// Action to take when budget is exceeded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BudgetExceededAction {
    /// Terminate the agent/team immediately.
    #[default]
    Terminate,
    /// Escalate to parent agent for handling.
    Escalate,
    /// Log a warning but continue execution.
    WarnAndContinue,
    /// Pause and wait for user confirmation.
    AskUser,
}

impl Default for TeamPolicy {
    fn default() -> Self {
        Self {
            max_depth: default_max_depth(),
            tool_whitelist: HashSet::new(),
            tool_blacklist: HashSet::new(),
            max_cost_per_agent: Some(1.0),
            max_tokens_per_agent: Some(50_000),
            max_turns_per_agent: default_max_turns(),
            total_budget_usd: Some(5.0),
            total_budget_tokens: Some(500_000),
            agent_timeout_secs: default_timeout_secs(),
            team_timeout_secs: default_team_timeout_secs(),
            budget_exceeded_action: BudgetExceededAction::default(),
            depth_tool_restrictions: HashMap::new(),
            allow_nested_teams: false,
            inherit_parent_permissions: true,
        }
    }
}

impl TeamPolicy {
    /// Create a new policy with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum nesting depth.
    pub fn with_max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth;
        self
    }

    /// Set the tool whitelist.
    pub fn with_tool_whitelist(mut self, tools: Vec<String>) -> Self {
        self.tool_whitelist = tools.into_iter().collect();
        self
    }

    /// Set the tool blacklist.
    pub fn with_tool_blacklist(mut self, tools: Vec<String>) -> Self {
        self.tool_blacklist = tools.into_iter().collect();
        self
    }

    /// Set the maximum cost per agent.
    pub fn with_max_cost_per_agent(mut self, cost: f64) -> Self {
        self.max_cost_per_agent = Some(cost);
        self
    }

    /// Set the maximum tokens per agent.
    pub fn with_max_tokens_per_agent(mut self, tokens: u64) -> Self {
        self.max_tokens_per_agent = Some(tokens);
        self
    }

    /// Set the maximum turns per agent.
    pub fn with_max_turns_per_agent(mut self, turns: u32) -> Self {
        self.max_turns_per_agent = turns;
        self
    }

    /// Set the total budget for the team.
    pub fn with_total_budget(mut self, budget: f64) -> Self {
        self.total_budget_usd = Some(budget);
        self
    }

    /// Set whether nested teams are allowed.
    pub fn with_nested_teams(mut self, allowed: bool) -> Self {
        self.allow_nested_teams = allowed;
        self
    }

    /// Check if a tool is allowed for a sub-agent at the given depth.
    pub fn is_tool_allowed(&self, tool_name: &str, depth: u32) -> bool {
        // First check if depth exceeds max
        if depth > self.max_depth {
            return false;
        }

        // Check depth-specific restrictions first
        if let Some(depth_tools) = self.depth_tool_restrictions.get(&depth) {
            // Check denied tools at this depth
            if depth_tools.denied_tools.contains(tool_name) {
                return false;
            }

            // If allowed_tools is non-empty, only those are permitted
            if !depth_tools.allowed_tools.is_empty() {
                return depth_tools.allowed_tools.contains(tool_name);
            }
        }

        // Check global blacklist
        if self.tool_blacklist.contains(tool_name) {
            return false;
        }

        // Check global whitelist
        if !self.tool_whitelist.is_empty() {
            return self.tool_whitelist.contains(tool_name);
        }

        // Empty whitelist means all tools are allowed (except those in blacklist)
        true
    }

    /// Filter a list of tools to only those allowed at the given depth.
    pub fn filter_tools(&self, tools: &[String], depth: u32) -> Vec<String> {
        tools
            .iter()
            .filter(|t| self.is_tool_allowed(t, depth))
            .cloned()
            .collect()
    }

    /// Get the maximum turns allowed at a given depth.
    pub fn max_turns_at_depth(&self, depth: u32) -> u32 {
        if let Some(depth_tools) = self.depth_tool_restrictions.get(&depth) {
            depth_tools.max_turns.unwrap_or(self.max_turns_per_agent)
        } else {
            self.max_turns_per_agent
        }
    }

    /// Get the maximum cost allowed at a given depth.
    pub fn max_cost_at_depth(&self, depth: u32) -> Option<f64> {
        if let Some(depth_tools) = self.depth_tool_restrictions.get(&depth) {
            depth_tools.max_cost_usd.or(self.max_cost_per_agent)
        } else {
            self.max_cost_per_agent
        }
    }

    /// Check if the given depth is within limits.
    pub fn is_depth_allowed(&self, depth: u32) -> bool {
        depth <= self.max_depth
    }

    /// Check if budget is exceeded.
    pub fn is_budget_exceeded(&self, current_cost: f64, current_tokens: u64) -> bool {
        if let Some(max_cost) = self.total_budget_usd {
            if current_cost >= max_cost {
                return true;
            }
        }
        if let Some(max_tokens) = self.total_budget_tokens {
            if current_tokens >= max_tokens {
                return true;
            }
        }
        false
    }

    /// Check if per-agent budget is exceeded.
    pub fn is_agent_budget_exceeded(&self, cost: f64, tokens: u64, depth: u32) -> bool {
        // Check per-agent cost
        if let Some(max_cost) = self.max_cost_at_depth(depth) {
            if cost >= max_cost {
                return true;
            }
        }

        // Check per-agent tokens
        if let Some(max_tokens) = self.max_tokens_per_agent {
            if tokens >= max_tokens {
                return true;
            }
        }

        false
    }

    /// Get a description of the policy.
    pub fn describe(&self) -> String {
        let mut parts = vec![];

        parts.push(format!("max_depth={}", self.max_depth));

        if !self.tool_whitelist.is_empty() {
            let mut tools: Vec<_> = self.tool_whitelist.iter().collect();
            tools.sort();
            parts.push(format!(
                "whitelist=[{}]",
                tools
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if !self.tool_blacklist.is_empty() {
            let mut tools: Vec<_> = self.tool_blacklist.iter().collect();
            tools.sort();
            parts.push(format!(
                "blacklist=[{}]",
                tools
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if let Some(cost) = self.max_cost_per_agent {
            parts.push(format!("max_cost_per_agent=${:.2}", cost));
        }

        if let Some(budget) = self.total_budget_usd {
            parts.push(format!("total_budget=${:.2}", budget));
        }

        parts.push(format!("max_turns={}", self.max_turns_per_agent));
        parts.push(format!("timeout={}s", self.agent_timeout_secs));

        format!("TeamPolicy({})", parts.join(", "))
    }

    /// Create a policy for read-only operations.
    pub fn read_only() -> Self {
        Self::default()
            .with_tool_whitelist(vec![
                "FileRead".into(),
                "Grep".into(),
                "Glob".into(),
                "Bash".into(),
            ])
            .with_max_turns_per_agent(5)
    }

    /// Create a policy for safe operations (read + write, no network).
    pub fn safe_tools() -> Self {
        Self::default()
            .with_tool_whitelist(vec![
                "FileRead".into(),
                "FileEdit".into(),
                "FileWrite".into(),
                "Grep".into(),
                "Glob".into(),
                "Bash".into(),
            ])
            .with_tool_blacklist(vec!["WebSearch".into(), "WebFetch".into()])
    }

    /// Create a policy with full access (all tools allowed).
    pub fn full_access() -> Self {
        Self {
            tool_whitelist: HashSet::new(),
            tool_blacklist: HashSet::new(),
            ..Self::default()
        }
    }
}

/// Per-agent execution policy derived from the team policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentPolicy {
    /// The team policy this is derived from.
    #[serde(skip)]
    team_policy: Option<TeamPolicy>,

    /// Depth of this agent in the nesting hierarchy.
    pub depth: u32,

    /// Tools allowed for this agent.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub allowed_tools: HashSet<String>,

    /// Tools denied for this agent.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub denied_tools: HashSet<String>,

    /// Maximum turns for this agent.
    pub max_turns: u32,

    /// Maximum cost for this agent in USD.
    pub max_cost_usd: Option<f64>,

    /// Maximum tokens for this agent.
    pub max_tokens: Option<u64>,

    /// Timeout for this agent in seconds.
    pub timeout_secs: u64,
}

impl AgentPolicy {
    /// Create an agent policy from a team policy.
    pub fn from_team_policy(team_policy: &TeamPolicy, depth: u32) -> Self {
        // Get depth-specific restrictions if any
        let depth_tools = team_policy.depth_tool_restrictions.get(&depth);

        // Determine allowed/denied tools
        let allowed_tools = if let Some(dt) = depth_tools {
            if dt.allowed_tools.is_empty() {
                team_policy.tool_whitelist.clone()
            } else {
                dt.allowed_tools.clone()
            }
        } else {
            team_policy.tool_whitelist.clone()
        };

        let denied_tools = if let Some(dt) = depth_tools {
            dt.denied_tools.clone()
        } else {
            team_policy.tool_blacklist.clone()
        };

        Self {
            team_policy: Some(team_policy.clone()),
            depth,
            allowed_tools,
            denied_tools,
            max_turns: team_policy.max_turns_at_depth(depth),
            max_cost_usd: team_policy.max_cost_at_depth(depth),
            max_tokens: team_policy.max_tokens_per_agent,
            timeout_secs: team_policy.agent_timeout_secs,
        }
    }

    /// Check if a tool is allowed.
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        // Check denied first
        if self.denied_tools.contains(tool_name) {
            return false;
        }

        // If allowed_tools is non-empty, check it
        if !self.allowed_tools.is_empty() {
            return self.allowed_tools.contains(tool_name);
        }

        // Empty allowed_tools means all tools are permitted (except denied)
        true
    }

    /// Filter tools to only allowed ones.
    pub fn filter_tools(&self, tools: &[String]) -> Vec<String> {
        tools
            .iter()
            .filter(|t| self.is_tool_allowed(t))
            .cloned()
            .collect()
    }

    /// Check if budget is exceeded.
    pub fn is_budget_exceeded(&self, cost: f64, tokens: u64) -> bool {
        if let Some(max_cost) = self.max_cost_usd {
            if cost >= max_cost {
                return true;
            }
        }
        if let Some(max_tokens) = self.max_tokens {
            if tokens >= max_tokens {
                return true;
            }
        }
        false
    }

    /// Get a description of this agent's policy.
    pub fn describe(&self) -> String {
        let mut parts = vec![];
        parts.push(format!("depth={}", self.depth));

        if !self.allowed_tools.is_empty() {
            let mut tools: Vec<_> = self.allowed_tools.iter().collect();
            tools.sort();
            parts.push(format!(
                "tools=[{}]",
                tools
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            parts.push("tools=all".to_string());
        }

        if !self.denied_tools.is_empty() {
            let mut tools: Vec<_> = self.denied_tools.iter().collect();
            tools.sort();
            parts.push(format!(
                "denied=[{}]",
                tools
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        parts.push(format!("max_turns={}", self.max_turns));

        if let Some(cost) = self.max_cost_usd {
            parts.push(format!("max_cost=${:.2}", cost));
        }

        parts.push(format!("timeout={}s", self.timeout_secs));

        format!("AgentPolicy({})", parts.join(", "))
    }
}

/// Result from a sub-agent execution with metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResult {
    /// ID of the agent that produced this result.
    pub agent_id: String,

    /// The text output from the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Whether execution was successful.
    pub success: bool,

    /// Error message if execution failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Token usage breakdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AgentUsage>,

    /// Cost of this execution in USD.
    #[serde(default)]
    pub cost_usd: f64,

    /// Duration of execution in milliseconds.
    #[serde(default)]
    pub duration_ms: u64,

    /// Number of turns taken.
    #[serde(default)]
    pub turns: u32,

    /// Tools used during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_used: Vec<String>,

    /// Files read during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_read: Vec<String>,

    /// Files written during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_written: Vec<String>,

    /// Commands executed during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands_executed: Vec<String>,

    /// Additional metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl AgentResult {
    /// Create a new successful result.
    pub fn success(agent_id: impl Into<String>, text: String) -> Self {
        Self {
            agent_id: agent_id.into(),
            text: Some(text),
            success: true,
            error: None,
            usage: None,
            cost_usd: 0.0,
            duration_ms: 0,
            turns: 0,
            tools_used: Vec::new(),
            files_read: Vec::new(),
            files_written: Vec::new(),
            commands_executed: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Create a new failed result.
    pub fn failure(agent_id: impl Into<String>, error: String) -> Self {
        Self {
            agent_id: agent_id.into(),
            text: None,
            success: false,
            error: Some(error),
            usage: None,
            cost_usd: 0.0,
            duration_ms: 0,
            turns: 0,
            tools_used: Vec::new(),
            files_read: Vec::new(),
            files_written: Vec::new(),
            commands_executed: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Set the usage information.
    pub fn with_usage(mut self, usage: AgentUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Set the cost.
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost_usd = cost;
        self
    }

    /// Set the duration.
    pub fn with_duration(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }

    /// Set the number of turns.
    pub fn with_turns(mut self, turns: u32) -> Self {
        self.turns = turns;
        self
    }

    /// Add a tool that was used.
    pub fn add_tool_used(&mut self, tool: String) {
        if !self.tools_used.contains(&tool) {
            self.tools_used.push(tool);
        }
    }

    /// Add a file that was read.
    pub fn add_file_read(&mut self, path: String) {
        if !self.files_read.contains(&path) {
            self.files_read.push(path);
        }
    }

    /// Add a file that was written.
    pub fn add_file_written(&mut self, path: String) {
        if !self.files_written.contains(&path) {
            self.files_written.push(path);
        }
    }

    /// Add a command that was executed.
    pub fn add_command(&mut self, command: String) {
        if !self.commands_executed.contains(&command) {
            self.commands_executed.push(command);
        }
    }

    /// Add metadata.
    pub fn add_metadata(&mut self, key: String, value: serde_json::Value) {
        self.metadata.insert(key, value);
    }
}

/// Token usage for an agent execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentUsage {
    /// Input tokens.
    pub input_tokens: u64,

    /// Output tokens.
    pub output_tokens: u64,

    /// Cache creation tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,

    /// Cache read tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
}

impl AgentUsage {
    /// Create new usage stats.
    pub fn new(input: u64, output: u64) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        }
    }

    /// Get total tokens.
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

impl Default for AgentUsage {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// Collected results from a team execution.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamResults {
    /// Team ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,

    /// Individual agent results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_results: Vec<AgentResult>,

    /// Combined text output from all agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combined_text: Option<String>,

    /// Total token usage.
    #[serde(default)]
    pub total_usage: AgentUsage,

    /// Total cost in USD.
    #[serde(default)]
    pub total_cost_usd: f64,

    /// Total duration in milliseconds.
    #[serde(default)]
    pub total_duration_ms: u64,

    /// Total turns across all agents.
    #[serde(default)]
    pub total_turns: u32,

    /// Number of successful agents.
    #[serde(default)]
    pub success_count: u32,

    /// Number of failed agents.
    #[serde(default)]
    pub failure_count: u32,

    /// All files read by any agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub all_files_read: Vec<String>,

    /// All files written by any agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub all_files_written: Vec<String>,

    /// All commands executed by any agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub all_commands: Vec<String>,

    /// All tools used by any agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub all_tools_used: Vec<String>,

    /// Additional metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl TeamResults {
    /// Create a new empty results collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with a team ID.
    pub fn with_team_id(team_id: impl Into<String>) -> Self {
        Self {
            team_id: Some(team_id.into()),
            ..Self::default()
        }
    }

    /// Add an agent result.
    pub fn add_result(&mut self, result: AgentResult) {
        // Update counts
        if result.success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }

        // Accumulate usage
        if let Some(usage) = &result.usage {
            self.total_usage.input_tokens += usage.input_tokens;
            self.total_usage.output_tokens += usage.output_tokens;
            if let Some(cache) = usage.cache_creation_tokens {
                self.total_usage.cache_creation_tokens =
                    Some(self.total_usage.cache_creation_tokens.unwrap_or(0) + cache);
            }
            if let Some(cache) = usage.cache_read_tokens {
                self.total_usage.cache_read_tokens =
                    Some(self.total_usage.cache_read_tokens.unwrap_or(0) + cache);
            }
        }

        // Accumulate totals
        self.total_cost_usd += result.cost_usd;
        self.total_duration_ms += result.duration_ms;
        self.total_turns += result.turns;

        // Merge file/command/tool lists
        for file in &result.files_read {
            if !self.all_files_read.contains(file) {
                self.all_files_read.push(file.clone());
            }
        }
        for file in &result.files_written {
            if !self.all_files_written.contains(file) {
                self.all_files_written.push(file.clone());
            }
        }
        for cmd in &result.commands_executed {
            if !self.all_commands.contains(cmd) {
                self.all_commands.push(cmd.clone());
            }
        }
        for tool in &result.tools_used {
            if !self.all_tools_used.contains(tool) {
                self.all_tools_used.push(tool.clone());
            }
        }

        // Add result
        self.agent_results.push(result);
    }

    /// Build combined text from all agent results.
    pub fn build_combined_text(&mut self) {
        let texts: Vec<_> = self
            .agent_results
            .iter()
            .filter_map(|r| r.text.as_ref())
            .map(|t| t.as_str())
            .collect();

        if !texts.is_empty() {
            self.combined_text = Some(texts.join("\n\n---\n\n"));
        }
    }

    /// Check if all agents succeeded.
    pub fn all_succeeded(&self) -> bool {
        self.failure_count == 0 && self.success_count > 0
    }

    /// Check if all agents failed.
    pub fn all_failed(&self) -> bool {
        self.success_count == 0 && self.failure_count > 0
    }

    /// Get the total number of agents.
    pub fn total_agents(&self) -> u32 {
        self.success_count + self.failure_count
    }

    /// Get a summary string.
    pub fn summary(&self) -> String {
        format!(
            "TeamResults: {} agents ({} succeeded, {} failed), {} tokens, ${:.4}, {} ms",
            self.total_agents(),
            self.success_count,
            self.failure_count,
            self.total_usage.total(),
            self.total_cost_usd,
            self.total_duration_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::engine::team::policy::AgentPolicy;
    use crate::engine::team::policy::DepthTools;
    use crate::engine::team::policy::TeamPolicy;
    #[test]
    fn test_team_policy_default() {
        let policy = TeamPolicy::default();
        assert_eq!(policy.max_depth, 3);
        assert!(policy.tool_whitelist.is_empty());
        assert!(policy.tool_blacklist.is_empty());
        assert!(policy.max_cost_per_agent.is_some());
        assert!(policy.total_budget_usd.is_some());
    }

    #[test]
    fn test_team_policy_tool_whitelist() {
        let policy =
            TeamPolicy::default().with_tool_whitelist(vec!["FileRead".into(), "Grep".into()]);

        assert!(policy.is_tool_allowed("FileRead", 0));
        assert!(policy.is_tool_allowed("Grep", 0));
        assert!(!policy.is_tool_allowed("WebSearch", 0));
    }

    #[test]
    fn test_team_policy_tool_blacklist() {
        let policy = TeamPolicy::default().with_tool_blacklist(vec!["WebSearch".into()]);

        // All tools allowed except WebSearch
        assert!(policy.is_tool_allowed("FileRead", 0));
        assert!(policy.is_tool_allowed("Bash", 0));
        assert!(!policy.is_tool_allowed("WebSearch", 0));
    }

    #[test]
    fn test_team_policy_depth_restrictions() {
        let mut policy = TeamPolicy::default();
        policy.depth_tool_restrictions.insert(
            1,
            DepthTools {
                depth: 1,
                allowed_tools: vec!["FileRead".into(), "Bash".into()].into_iter().collect(),
                denied_tools: Default::default(),
                max_turns: Some(5),
                max_cost_usd: Some(0.5),
            },
        );

        // Depth 0: all tools allowed
        assert!(policy.is_tool_allowed("WebSearch", 0));

        // Depth 1: only FileRead and Bash allowed
        assert!(policy.is_tool_allowed("FileRead", 1));
        assert!(policy.is_tool_allowed("Bash", 1));
        assert!(!policy.is_tool_allowed("WebSearch", 1));

        // Depth 2: back to default (all allowed)
        assert!(policy.is_tool_allowed("WebSearch", 2));
    }

    #[test]
    fn test_team_policy_max_depth() {
        let policy = TeamPolicy::default().with_max_depth(2);

        assert!(policy.is_depth_allowed(0));
        assert!(policy.is_depth_allowed(1));
        assert!(policy.is_depth_allowed(2));
        assert!(!policy.is_depth_allowed(3));
    }

    #[test]
    fn test_team_policy_budget_exceeded() {
        let policy = TeamPolicy::default().with_total_budget(10.0);

        assert!(!policy.is_budget_exceeded(5.0, 0));
        assert!(policy.is_budget_exceeded(10.0, 0));
        assert!(policy.is_budget_exceeded(15.0, 0));
    }

    #[test]
    fn test_agent_policy_from_team_policy() {
        let team_policy = TeamPolicy::default()
            .with_tool_whitelist(vec!["FileRead".into(), "Grep".into()])
            .with_max_turns_per_agent(5);

        let agent_policy = AgentPolicy::from_team_policy(&team_policy, 1);

        assert_eq!(agent_policy.depth, 1);
        assert!(agent_policy.is_tool_allowed("FileRead"));
        assert!(!agent_policy.is_tool_allowed("WebSearch"));
        assert_eq!(agent_policy.max_turns, 5);
    }

    #[test]
    fn test_agent_result_success() {
        let result = AgentResult::success("agent-1", "Task completed".to_string());

        assert_eq!(result.agent_id, "agent-1");
        assert!(result.success);
        assert_eq!(result.text, Some("Task completed".to_string()));
        assert!(result.error.is_none());
    }

    #[test]
    fn test_agent_result_failure() {
        let result = AgentResult::failure("agent-1", "Something went wrong".to_string());

        assert_eq!(result.agent_id, "agent-1");
        assert!(!result.success);
        assert!(result.text.is_none());
        assert_eq!(result.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_agent_result_add_tools_files() {
        let mut result = AgentResult::success("agent-1", "Done".to_string());

        result.add_tool_used("FileRead".to_string());
        result.add_tool_used("Grep".to_string());
        result.add_tool_used("FileRead".to_string()); // duplicate should not be added

        result.add_file_read("/src/main.rs".to_string());
        result.add_file_written("/src/lib.rs".to_string());

        assert_eq!(result.tools_used, vec!["FileRead", "Grep"]);
        assert_eq!(result.files_read, vec!["/src/main.rs"]);
        assert_eq!(result.files_written, vec!["/src/lib.rs"]);
    }

    #[test]
    fn test_team_results() {
        let mut results = TeamResults::with_team_id("team-1");

        let result1 = AgentResult::success("agent-1", "Result 1".to_string())
            .with_usage(AgentUsage::new(100, 50))
            .with_cost(0.01)
            .with_duration(1000);

        let result2 = AgentResult::success("agent-2", "Result 2".to_string())
            .with_usage(AgentUsage::new(200, 100))
            .with_cost(0.02)
            .with_duration(2000);

        results.add_result(result1);
        results.add_result(result2);

        assert_eq!(results.total_agents(), 2);
        assert_eq!(results.success_count, 2);
        assert_eq!(results.failure_count, 0);
        assert!(results.all_succeeded());
        assert_eq!(results.total_usage.input_tokens, 300);
        assert_eq!(results.total_usage.output_tokens, 150);
        assert!((results.total_cost_usd - 0.03).abs() < 1e-10);
        assert_eq!(results.total_duration_ms, 3000);
    }

    #[test]
    fn test_team_results_mixed() {
        let mut results = TeamResults::new();

        results.add_result(AgentResult::success("agent-1", "OK".to_string()));
        results.add_result(AgentResult::failure("agent-2", "Failed".to_string()));

        assert!(!results.all_succeeded());
        assert!(!results.all_failed());
        assert_eq!(results.success_count, 1);
        assert_eq!(results.failure_count, 1);
    }

    #[test]
    fn test_read_only_policy() {
        let policy = TeamPolicy::read_only();

        assert!(policy.is_tool_allowed("FileRead", 0));
        assert!(policy.is_tool_allowed("Grep", 0));
        assert!(policy.is_tool_allowed("Glob", 0));
        assert!(policy.is_tool_allowed("Bash", 0));
        assert!(!policy.is_tool_allowed("FileWrite", 0));
        assert!(!policy.is_tool_allowed("FileEdit", 0));
    }

    #[test]
    fn test_safe_tools_policy() {
        let policy = TeamPolicy::safe_tools();

        assert!(policy.is_tool_allowed("FileRead", 0));
        assert!(policy.is_tool_allowed("FileWrite", 0));
        assert!(policy.is_tool_allowed("FileEdit", 0));
        assert!(policy.is_tool_allowed("Bash", 0));
        assert!(!policy.is_tool_allowed("WebSearch", 0));
        assert!(!policy.is_tool_allowed("WebFetch", 0));
    }

    #[test]
    fn test_full_access_policy() {
        let policy = TeamPolicy::full_access();

        assert!(policy.is_tool_allowed("FileRead", 0));
        assert!(policy.is_tool_allowed("WebSearch", 0));
        assert!(policy.is_tool_allowed("Anything", 0));
    }

    #[test]
    fn test_policy_serialization() {
        let policy = TeamPolicy::default()
            .with_tool_whitelist(vec!["FileRead".into()])
            .with_max_cost_per_agent(0.5);

        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: TeamPolicy = serde_json::from_str(&json).unwrap();

        assert!(deserialized.tool_whitelist.contains("FileRead"));
        assert_eq!(deserialized.max_cost_per_agent, Some(0.5));
    }

    #[test]
    fn test_agent_result_serialization() {
        let result = AgentResult::success("agent-1", "Done".to_string())
            .with_cost(0.05)
            .with_duration(1500);

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: AgentResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.agent_id, "agent-1");
        assert!(deserialized.success);
        assert!((deserialized.cost_usd - 0.05).abs() < 1e-10);
        assert_eq!(deserialized.duration_ms, 1500);
    }

    #[test]
    fn test_team_results_build_combined_text() {
        let mut results = TeamResults::new();

        results.add_result(AgentResult::success("a", "First result".to_string()));
        results.add_result(AgentResult::success("b", "Second result".to_string()));
        results.build_combined_text();

        let combined = results.combined_text.unwrap();
        assert!(combined.contains("First result"));
        assert!(combined.contains("Second result"));
    }

    #[test]
    fn test_agent_usage_total() {
        let usage = AgentUsage::new(100, 50);
        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn test_team_policy_describe() {
        let policy = TeamPolicy::default()
            .with_tool_whitelist(vec!["FileRead".into()])
            .with_max_turns_per_agent(5);

        let desc = policy.describe();
        assert!(desc.contains("max_depth=3"));
        assert!(desc.contains("max_turns=5"));
    }
}
