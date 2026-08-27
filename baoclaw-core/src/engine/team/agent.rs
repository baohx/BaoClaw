//! Sub-Agent execution implementation.
//!
//! This module implements the execution logic for individual sub-agents within a team,
//! including:
//! - Tool permission inheritance and restriction
//! - Real-time budget control and tracking
//! - Comprehensive result collection
//!
//! # Key Components
//!
//! - `SubAgentExecutor` - Executes a single sub-agent with policy enforcement
//! - `ExecutionTracker` - Tracks tool usage, files, and commands in real-time
//! - `BudgetEnforcer` - Monitors and enforces budget limits during execution

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

use crate::api::unified::UnifiedClient;
use crate::engine::query_engine::{EngineEvent, QueryEngine, QueryEngineConfig, ThinkingConfig};
use crate::engine::team::policy::{AgentPolicy, AgentResult, AgentUsage, BudgetExceededAction};
use crate::tools::trait_def::Tool;
use serde::{Deserialize, Serialize};

/// Error type for sub-agent execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubAgentError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for SubAgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]: {}", self.code, self.message)
    }
}

impl std::error::Error for SubAgentError {}

/// Tracks execution state during sub-agent runs.
#[derive(Clone, Debug, Default)]
pub struct ExecutionTracker {
    /// Tools used during execution.
    pub tools_used: Vec<String>,
    /// Files read during execution.
    pub files_read: Vec<String>,
    /// Files written during execution.
    pub files_written: Vec<String>,
    /// Commands executed during execution.
    pub commands_executed: Vec<String>,
    /// Number of turns completed.
    pub turns: u32,
    /// Total input tokens.
    pub total_input_tokens: u64,
    /// Total output tokens.
    pub total_output_tokens: u64,
    /// Total cost in USD.
    pub total_cost: f64,
}

impl ExecutionTracker {
    /// Create a new empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a tool use.
    pub fn record_tool_use(&mut self, tool_name: &str) {
        if !self.tools_used.contains(&tool_name.to_string()) {
            self.tools_used.push(tool_name.to_string());
        }
    }

    /// Record a file read.
    pub fn record_file_read(&mut self, path: &str) {
        if !self.files_read.contains(&path.to_string()) {
            self.files_read.push(path.to_string());
        }
    }

    /// Record a file write.
    pub fn record_file_write(&mut self, path: &str) {
        if !self.files_written.contains(&path.to_string()) {
            self.files_written.push(path.to_string());
        }
    }

    /// Record a command execution.
    pub fn record_command(&mut self, command: &str) {
        if !self.commands_executed.contains(&command.to_string()) {
            self.commands_executed.push(command.to_string());
        }
    }

    /// Update token usage.
    pub fn update_usage(&mut self, input: u64, output: u64) {
        self.total_input_tokens += input;
        self.total_output_tokens += output;
    }

    /// Update cost.
    pub fn update_cost(&mut self, cost: f64) {
        self.total_cost += cost;
    }

    /// Increment turn count.
    pub fn increment_turn(&mut self) {
        self.turns += 1;
    }

    /// Convert to AgentUsage.
    pub fn to_usage(&self) -> AgentUsage {
        AgentUsage::new(self.total_input_tokens, self.total_output_tokens)
    }
}

/// Budget enforcement state.
#[derive(Clone, Debug)]
pub struct BudgetEnforcer {
    /// Maximum cost allowed.
    pub max_cost: Option<f64>,
    /// Maximum tokens allowed.
    pub max_tokens: Option<u64>,
    /// Maximum turns allowed.
    pub max_turns: Option<u32>,
    /// Action to take on budget exceeded.
    pub exceeded_action: BudgetExceededAction,
    /// Current cost.
    pub current_cost: f64,
    /// Current tokens.
    pub current_tokens: u64,
    /// Current turns.
    pub current_turns: u32,
    /// Whether budget has been exceeded.
    pub is_exceeded: bool,
}

impl BudgetEnforcer {
    /// Create a new budget enforcer from an agent policy.
    pub fn from_policy(policy: &AgentPolicy) -> Self {
        Self {
            max_cost: policy.max_cost_usd,
            max_tokens: policy.max_tokens,
            max_turns: Some(policy.max_turns),
            exceeded_action: BudgetExceededAction::Terminate, // Default for sub-agents
            current_cost: 0.0,
            current_tokens: 0,
            current_turns: 0,
            is_exceeded: false,
        }
    }

    /// Check cost budget.
    pub fn check_cost(&self) -> bool {
        if let Some(max) = self.max_cost {
            self.current_cost < max
        } else {
            true
        }
    }

    /// Check token budget.
    pub fn check_tokens(&self) -> bool {
        if let Some(max) = self.max_tokens {
            self.current_tokens < max
        } else {
            true
        }
    }

    /// Check turn budget.
    pub fn check_turns(&self) -> bool {
        if let Some(max) = self.max_turns {
            self.current_turns < max
        } else {
            true
        }
    }

    /// Check all budgets.
    pub fn check_all(&self) -> bool {
        self.check_cost() && self.check_tokens() && self.check_turns()
    }

    /// Update cost and check.
    pub fn update_cost(&mut self, cost: f64) -> bool {
        self.current_cost += cost;
        if !self.check_cost() {
            self.is_exceeded = true;
            return false;
        }
        true
    }

    /// Update tokens and check.
    pub fn update_tokens(&mut self, input: u64, output: u64) -> bool {
        self.current_tokens += input + output;
        if !self.check_tokens() {
            self.is_exceeded = true;
            return false;
        }
        true
    }

    /// Increment turn and check.
    pub fn increment_turn(&mut self) -> bool {
        self.current_turns += 1;
        if !self.check_turns() {
            self.is_exceeded = true;
            return false;
        }
        true
    }

    /// Get budget status message.
    pub fn status_message(&self) -> String {
        let mut parts = vec![];
        if let Some(max) = self.max_cost {
            parts.push(format!("cost: ${:.4}/${:.2}", self.current_cost, max));
        }
        if let Some(max) = self.max_tokens {
            parts.push(format!("tokens: {}/{}", self.current_tokens, max));
        }
        if let Some(max) = self.max_turns {
            parts.push(format!("turns: {}/{}", self.current_turns, max));
        }
        if parts.is_empty() {
            "no budget limits".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Sub-agent executor with policy enforcement.
pub struct SubAgentExecutor {
    /// API client for LLM calls.
    api_client: Arc<UnifiedClient>,
    /// Available tools.
    tools: Vec<Arc<dyn Tool>>,
    /// Working directory.
    cwd: PathBuf,
    /// Model to use.
    model: String,
    /// Agent policy for permissions and budget.
    policy: AgentPolicy,
    /// Agent ID for tracking.
    agent_id: String,
}

impl SubAgentExecutor {
    /// Create a new sub-agent executor.
    pub fn new(
        api_client: Arc<UnifiedClient>,
        tools: Vec<Arc<dyn Tool>>,
        cwd: PathBuf,
        model: String,
        policy: AgentPolicy,
        agent_id: String,
    ) -> Self {
        Self {
            api_client,
            tools,
            cwd,
            model,
            policy,
            agent_id,
        }
    }

    /// Execute the sub-agent with the given prompt.
    pub async fn execute(
        self,
        prompt: String,
        mut abort_rx: watch::Receiver<bool>,
    ) -> Result<AgentResult, SubAgentError> {
        let start_time = std::time::Instant::now();

        // Initialize tracking
        let tracker = Arc::new(RwLock::new(ExecutionTracker::new()));
        let budget = Arc::new(RwLock::new(BudgetEnforcer::from_policy(&self.policy)));

        // Filter tools based on policy
        let filtered_tools = self.filter_tools();

        // Create engine config
        let config = QueryEngineConfig {
            cwd: self.cwd.clone(),
            tools: filtered_tools,
            api_client: Arc::clone(&self.api_client),
            model: self.model.clone(),
            thinking_config: ThinkingConfig::Disabled,
            max_turns: Some(self.policy.max_turns),
            max_budget_usd: self.policy.max_cost_usd,
            verbose: false,
            custom_system_prompt: Some(
                "You are a sub-agent. Complete the given task efficiently and report results.\n\
                 Be thorough but concise. Focus on the specific task assigned to you."
                    .to_string(),
            ),
            append_system_prompt: None,
            session_id: None,
            fallback_models: vec![],
            max_retries_per_model: 2,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
            parent_turn_id: None,
            agent_label: Some("sub-agent".to_string()),
            session_memory: None,
            file_cache: None,
            tool_result_store: None,
            hook_manager: None,
        };

        // Create engine and submit prompt
        let mut engine = QueryEngine::new(config);
        let mut rx = engine.submit_message(prompt).await;

        let mut final_text = String::new();

        // Process events
        loop {
            tokio::select! {
                // Check for abort
                _ = abort_rx.changed() => {
                    if *abort_rx.borrow() {
                        engine.abort();
                        return Err(SubAgentError {
                            code: "aborted".to_string(),
                            message: "Agent execution was aborted by user".to_string(),
                        });
                    }
                }

                // Process events
                event = rx.recv() => {
                    match event {
                        Some(EngineEvent::AssistantChunk { content, .. }) => {
                            final_text.push_str(&content);
                        }
                        Some(EngineEvent::ToolUse { tool_name, input, .. }) => {
                            // Track tool usage
                            tracker.write().await.record_tool_use(&tool_name);

                            // Track specific tool operations
                            Self::track_tool_operation(&tracker, &tool_name, &input).await;
                        }
                        Some(EngineEvent::TurnEnd { turn_id, input_tokens, output_tokens, .. }) => {
                            // Update budget tracking
                            let mut b = budget.write().await;
                            b.update_tokens(input_tokens, output_tokens);
                            b.increment_turn();

                            // Update tracker
                            let mut t = tracker.write().await;
                            t.update_usage(input_tokens, output_tokens);
                            t.turns = turn_id + 1;

                            // Check budget
                            if b.is_exceeded {
                                engine.abort();
                                return Err(SubAgentError {
                                    code: "budget_exceeded".to_string(),
                                    message: format!(
                                        "Budget exceeded: {}",
                                        b.status_message()
                                    ),
                                });
                            }
                        }
                        Some(EngineEvent::Result(result)) => {
                            if let Some(text) = result.text {
                                final_text = text;
                            }

                            // Final update to tracker
                            let mut t = tracker.write().await;
                            t.update_usage(result.usage.input_tokens, result.usage.output_tokens);
                            t.update_cost(result.total_cost_usd);
                            t.turns = result.num_turns;

                            break;
                        }
                        Some(EngineEvent::Error(err)) => {
                            return Err(SubAgentError {
                                code: err.code,
                                message: err.message,
                            });
                        }
                        None => break,
                        _ => {}
                    }
                }
            }
        }

        // Build final result
        let tracker_guard = tracker.read().await;
        let duration_ms = start_time.elapsed().as_millis() as u64;

        let result = AgentResult::success(self.agent_id.clone(), final_text)
            .with_usage(tracker_guard.to_usage())
            .with_cost(tracker_guard.total_cost)
            .with_duration(duration_ms)
            .with_turns(tracker_guard.turns);

        // Add tracked data to result
        let mut result = result;
        result.tools_used = tracker_guard.tools_used.clone();
        result.files_read = tracker_guard.files_read.clone();
        result.files_written = tracker_guard.files_written.clone();
        result.commands_executed = tracker_guard.commands_executed.clone();

        Ok(result)
    }

    /// Filter tools based on policy.
    fn filter_tools(&self) -> Vec<Arc<dyn Tool>> {
        let allowed_names: HashSet<String> = self
            .tools
            .iter()
            .filter(|t| self.policy.is_tool_allowed(t.name()))
            .map(|t| t.name().to_string())
            .collect();

        self.tools
            .iter()
            .filter(|t| allowed_names.contains(t.name()))
            .cloned()
            .collect()
    }

    /// Track tool-specific operations.
    async fn track_tool_operation(
        tracker: &Arc<RwLock<ExecutionTracker>>,
        tool_name: &str,
        input: &serde_json::Value,
    ) {
        let mut tracker = tracker.write().await;

        match tool_name {
            "FileRead" | "ReadFile" | "read_file" => {
                if let Some(path) = input.get("path").and_then(|p| p.as_str()) {
                    tracker.record_file_read(path);
                }
            }
            "FileWrite" | "WriteFile" | "write_file" => {
                if let Some(path) = input.get("path").and_then(|p| p.as_str()) {
                    tracker.record_file_write(path);
                }
            }
            "FileEdit" | "EditFile" | "edit_file" | "str_replace" => {
                if let Some(path) = input.get("path").and_then(|p| p.as_str()) {
                    tracker.record_file_write(path);
                }
            }
            "Bash" | "ExecuteBash" | "execute_bash" => {
                if let Some(cmd) = input.get("command").and_then(|c| c.as_str()) {
                    tracker.record_command(cmd);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::engine::team::agent::BudgetEnforcer;
    use crate::engine::team::policy::AgentPolicy;
    use crate::engine::team::policy::TeamPolicy;
    #[test]
    fn test_execution_tracker() {
        let mut tracker = ExecutionTracker::new();

        tracker.record_tool_use("FileRead");
        tracker.record_tool_use("Grep");
        tracker.record_tool_use("FileRead"); // duplicate

        tracker.record_file_read("/src/main.rs");
        tracker.record_file_write("/src/lib.rs");
        tracker.record_command("npm test");

        tracker.update_usage(100, 50);
        tracker.update_cost(0.01);
        tracker.increment_turn();

        assert_eq!(tracker.tools_used, vec!["FileRead", "Grep"]);
        assert_eq!(tracker.files_read, vec!["/src/main.rs"]);
        assert_eq!(tracker.files_written, vec!["/src/lib.rs"]);
        assert_eq!(tracker.commands_executed, vec!["npm test"]);
        assert_eq!(tracker.total_input_tokens, 100);
        assert_eq!(tracker.total_output_tokens, 50);
        assert_eq!(tracker.total_cost, 0.01);
        assert_eq!(tracker.turns, 1);
    }

    #[test]
    fn test_budget_enforcer() {
        let team_policy = TeamPolicy::default()
            .with_max_turns_per_agent(5)
            .with_max_cost_per_agent(1.0)
            .with_max_tokens_per_agent(10000);

        let policy = AgentPolicy::from_team_policy(&team_policy, 1);

        let mut enforcer = BudgetEnforcer::from_policy(&policy);

        // Initial state
        assert!(enforcer.check_all());
        assert!(!enforcer.is_exceeded);

        // Update within limits
        assert!(enforcer.update_cost(0.5));
        assert!(enforcer.update_tokens(1000, 500));
        assert!(enforcer.increment_turn());

        // Still within limits
        assert!(enforcer.check_all());
        assert!(!enforcer.is_exceeded);

        // Exceed cost
        assert!(!enforcer.update_cost(0.6)); // total: 1.1 > 1.0
        assert!(enforcer.is_exceeded);
    }

    #[test]
    fn test_budget_enforcer_status_message() {
        let team_policy = TeamPolicy::default()
            .with_max_turns_per_agent(5)
            .with_max_cost_per_agent(1.0)
            .with_max_tokens_per_agent(10000);

        let policy = AgentPolicy::from_team_policy(&team_policy, 1);
        let enforcer = BudgetEnforcer::from_policy(&policy);
        let msg = enforcer.status_message();

        assert!(msg.contains("cost:"));
        assert!(msg.contains("tokens:"));
        assert!(msg.contains("turns:"));
    }

    #[test]
    fn test_execution_tracker_to_usage() {
        let mut tracker = ExecutionTracker::new();
        tracker.update_usage(100, 50);

        let usage = tracker.to_usage();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total(), 150);
    }
}
