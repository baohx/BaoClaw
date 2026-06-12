//! Core types for the Sub-Agent Team system.
//!
//! This module defines the fundamental types used for managing teams of
//! sub-agents that can execute in parallel, sequential, or DAG mode.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Execution mode for a team of agents.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TeamMode {
    /// All agents execute simultaneously.
    Parallel,
    /// Agents execute one after another in sequence.
    Sequence,
    /// Agents execute according to a directed acyclic graph (DAG).
    Dag,
}

impl Default for TeamMode {
    fn default() -> Self {
        Self::Parallel
    }
}

impl std::fmt::Display for TeamMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parallel => write!(f, "parallel"),
            Self::Sequence => write!(f, "sequence"),
            Self::Dag => write!(f, "dag"),
        }
    }
}

impl std::str::FromStr for TeamMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "parallel" => Ok(Self::Parallel),
            "sequence" | "sequential" => Ok(Self::Sequence),
            "dag" => Ok(Self::Dag),
            _ => Err(format!("Unknown team mode: {}", s)),
        }
    }
}

/// Status of a team execution.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TeamStatus {
    /// Team is created but not yet started.
    Pending,
    /// Team is currently executing.
    Running,
    /// Team completed successfully.
    Completed,
    /// Team execution failed.
    Failed,
    /// Team was aborted by user.
    Aborted,
}

impl Default for TeamStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl std::fmt::Display for TeamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Aborted => write!(f, "aborted"),
        }
    }
}

/// Status of an individual sub-agent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentStatus {
    /// Agent is waiting to start.
    Pending,
    /// Agent is currently executing.
    Running,
    /// Agent completed successfully.
    Completed,
    /// Agent execution failed.
    Failed,
    /// Agent was skipped (e.g., in DAG mode when dependency failed).
    Skipped,
}

impl Default for SubAgentStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl std::fmt::Display for SubAgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

/// A sub-agent within a team.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubAgent {
    /// Unique identifier within the team.
    pub id: String,

    /// Human-readable name or description.
    pub name: String,

    /// The task prompt for this agent to execute.
    pub prompt: String,

    /// Current status of this agent.
    #[serde(default)]
    pub status: SubAgentStatus,

    /// Dependencies in DAG mode (IDs of agents this one depends on).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,

    /// Result from the agent execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    /// Error message if the agent failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Token usage for this agent.
    #[serde(default)]
    pub tokens_used: u64,

    /// Cost for this agent execution in USD.
    #[serde(default)]
    pub cost_usd: f64,

    /// Additional metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl SubAgent {
    /// Create a new sub-agent with the given ID and prompt.
    pub fn new(id: impl Into<String>, prompt: impl Into<String>) -> Self {
        let id = id.into();
        let prompt = prompt.into();
        Self {
            id: id.clone(),
            name: id,
            prompt,
            status: SubAgentStatus::default(),
            dependencies: Vec::new(),
            result: None,
            error: None,
            tokens_used: 0,
            cost_usd: 0.0,
            metadata: HashMap::new(),
        }
    }

    /// Set the name for this agent.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Add a dependency for DAG execution.
    pub fn with_dependency(mut self, dep_id: impl Into<String>) -> Self {
        self.dependencies.push(dep_id.into());
        self
    }

    /// Mark this agent as running.
    pub fn start(&mut self) {
        self.status = SubAgentStatus::Running;
    }

    /// Mark this agent as completed with a result.
    pub fn complete(&mut self, result: String, tokens_used: u64, cost_usd: f64) {
        self.status = SubAgentStatus::Completed;
        self.result = Some(result);
        self.tokens_used = tokens_used;
        self.cost_usd = cost_usd;
    }

    /// Mark this agent as failed with an error.
    pub fn fail(&mut self, error: String) {
        self.status = SubAgentStatus::Failed;
        self.error = Some(error);
    }

    /// Mark this agent as skipped.
    pub fn skip(&mut self, reason: String) {
        self.status = SubAgentStatus::Skipped;
        self.metadata.insert("skip_reason".to_string(), reason);
    }

    /// Check if this agent is ready to run (all dependencies satisfied).
    pub fn is_ready(&self, completed: &std::collections::HashSet<&str>) -> bool {
        self.status == SubAgentStatus::Pending
            && self.dependencies.iter().all(|dep| completed.contains(dep.as_str()))
    }
}

/// Budget configuration for a team.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamBudget {
    /// Maximum total cost in USD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,

    /// Maximum total tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,

    /// Maximum execution time in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_time_secs: Option<u64>,
}

impl Default for TeamBudget {
    fn default() -> Self {
        Self {
            max_cost_usd: None,
            max_tokens: None,
            max_time_secs: Some(600), // 10 minutes default
        }
    }
}

/// Shared state for inter-agent communication.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SharedState {
    /// Key-value store for shared data.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub data: HashMap<String, serde_json::Value>,

    /// Progress reports from agents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub progress: Vec<ProgressReport>,
}

impl SharedState {
    /// Create a new empty shared state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a value in the shared state.
    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.data.insert(key.into(), value);
    }

    /// Get a value from the shared state.
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.data.get(key)
    }

    /// Report progress from an agent.
    pub fn report_progress(&mut self, agent_id: impl Into<String>, message: impl Into<String>, progress: f64) {
        self.progress.push(ProgressReport {
            agent_id: agent_id.into(),
            message: message.into(),
            progress,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }
}

/// A progress report from an agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressReport {
    /// ID of the agent reporting.
    pub agent_id: String,

    /// Progress message.
    pub message: String,

    /// Progress as a fraction (0.0 to 1.0).
    pub progress: f64,

    /// Timestamp of the report.
    pub timestamp: String,
}

/// A team of sub-agents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTeam {
    /// Unique identifier for this team.
    pub id: String,

    /// Human-readable name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The overall task prompt for the team.
    pub task: String,

    /// Execution mode.
    #[serde(default)]
    pub mode: TeamMode,

    /// Current status.
    #[serde(default)]
    pub status: TeamStatus,

    /// Sub-agents in this team.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<SubAgent>,

    /// Budget constraints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<TeamBudget>,

    /// Shared state for inter-agent communication.
    #[serde(default)]
    pub shared_state: SharedState,

    /// Creation timestamp (ISO 8601).
    pub created_at: String,

    /// Start timestamp (ISO 8601).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,

    /// Completion timestamp (ISO 8601).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,

    /// Total tokens used by all agents.
    #[serde(default)]
    pub total_tokens: u64,

    /// Total cost in USD.
    #[serde(default)]
    pub total_cost_usd: f64,

    /// Working directory for the team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

impl AgentTeam {
    /// Create a new team with the given ID and task.
    pub fn new(id: impl Into<String>, task: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: None,
            task: task.into(),
            mode: TeamMode::default(),
            status: TeamStatus::default(),
            agents: Vec::new(),
            budget: None,
            shared_state: SharedState::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
            total_tokens: 0,
            total_cost_usd: 0.0,
            cwd: None,
        }
    }

    /// Set the team name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the execution mode.
    pub fn with_mode(mut self, mode: TeamMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the budget constraints.
    pub fn with_budget(mut self, budget: TeamBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Set the working directory.
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Add a sub-agent to the team.
    pub fn add_agent(&mut self, agent: SubAgent) {
        self.agents.push(agent);
    }

    /// Create parallel agents with the same prompt.
    pub fn create_parallel_agents(&mut self, count: usize, prompt_prefix: &str) {
        for i in 0..count {
            let agent = SubAgent::new(format!("agent-{}", i), format!("{} (part {})", prompt_prefix, i + 1));
            self.agents.push(agent);
        }
    }

    /// Get an agent by ID.
    pub fn get_agent(&self, id: &str) -> Option<&SubAgent> {
        self.agents.iter().find(|a| a.id == id)
    }

    /// Get a mutable agent by ID.
    pub fn get_agent_mut(&mut self, id: &str) -> Option<&mut SubAgent> {
        self.agents.iter_mut().find(|a| a.id == id)
    }

    /// Start the team execution.
    pub fn start(&mut self) {
        self.status = TeamStatus::Running;
        self.started_at = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Complete the team execution.
    pub fn complete(&mut self) {
        self.status = TeamStatus::Completed;
        self.completed_at = Some(chrono::Utc::now().to_rfc3339());
        self.calculate_totals();
    }

    /// Fail the team execution.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = TeamStatus::Failed;
        self.completed_at = Some(chrono::Utc::now().to_rfc3339());
        self.shared_state.set("error", serde_json::json!(error.into()));
    }

    /// Abort the team execution.
    pub fn abort(&mut self, reason: impl Into<String>) {
        self.status = TeamStatus::Aborted;
        self.completed_at = Some(chrono::Utc::now().to_rfc3339());
        self.shared_state.set("abort_reason", serde_json::json!(reason.into()));
    }

    /// Calculate total tokens and cost from all agents.
    pub fn calculate_totals(&mut self) {
        self.total_tokens = self.agents.iter().map(|a| a.tokens_used).sum();
        self.total_cost_usd = self.agents.iter().map(|a| a.cost_usd).sum();
    }

    /// Check if budget is exceeded.
    pub fn is_budget_exceeded(&self) -> bool {
        if let Some(budget) = &self.budget {
            if let Some(max_cost) = budget.max_cost_usd {
                if self.total_cost_usd >= max_cost {
                    return true;
                }
            }
            if let Some(max_tokens) = budget.max_tokens {
                if self.total_tokens >= max_tokens {
                    return true;
                }
            }
        }
        false
    }

    /// Get completed agent IDs.
    pub fn completed_agent_ids(&self) -> std::collections::HashSet<&str> {
        self.agents
            .iter()
            .filter(|a| a.status == SubAgentStatus::Completed)
            .map(|a| a.id.as_str())
            .collect()
    }

    /// Get agents that are ready to run (for DAG execution).
    pub fn ready_agents(&self) -> Vec<&SubAgent> {
        let completed = self.completed_agent_ids();
        self.agents
            .iter()
            .filter(|a| a.is_ready(&completed))
            .collect()
    }

    /// Get results from all completed agents.
    pub fn collect_results(&self) -> HashMap<String, String> {
        self.agents
            .iter()
            .filter(|a| a.status == SubAgentStatus::Completed)
            .filter_map(|a| a.result.as_ref().map(|r| (a.id.clone(), r.clone())))
            .collect()
    }

    /// Get a summary of the team status.
    pub fn summary(&self) -> TeamSummary {
        let mut summary = TeamSummary::default();
        summary.total_agents = self.agents.len();
        for agent in &self.agents {
            match agent.status {
                SubAgentStatus::Pending => summary.pending_count += 1,
                SubAgentStatus::Running => summary.running_count += 1,
                SubAgentStatus::Completed => summary.completed_count += 1,
                SubAgentStatus::Failed => summary.failed_count += 1,
                SubAgentStatus::Skipped => summary.skipped_count += 1,
            }
        }
        summary.total_tokens = self.total_tokens;
        summary.total_cost_usd = self.total_cost_usd;
        summary
    }
}

/// Summary of a team's status.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamSummary {
    /// Total number of agents.
    pub total_agents: usize,
    /// Number of pending agents.
    pub pending_count: usize,
    /// Number of running agents.
    pub running_count: usize,
    /// Number of completed agents.
    pub completed_count: usize,
    /// Number of failed agents.
    pub failed_count: usize,
    /// Number of skipped agents.
    pub skipped_count: usize,
    /// Total tokens used.
    pub total_tokens: u64,
    /// Total cost in USD.
    pub total_cost_usd: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_team_mode_from_str() {
        assert_eq!(TeamMode::from_str("parallel").unwrap(), TeamMode::Parallel);
        assert_eq!(TeamMode::from_str("sequence").unwrap(), TeamMode::Sequence);
        assert_eq!(TeamMode::from_str("sequential").unwrap(), TeamMode::Sequence);
        assert_eq!(TeamMode::from_str("dag").unwrap(), TeamMode::Dag);
        assert!(TeamMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_team_mode_display() {
        assert_eq!(TeamMode::Parallel.to_string(), "parallel");
        assert_eq!(TeamMode::Sequence.to_string(), "sequence");
        assert_eq!(TeamMode::Dag.to_string(), "dag");
    }

    #[test]
    fn test_team_status_display() {
        assert_eq!(TeamStatus::Pending.to_string(), "pending");
        assert_eq!(TeamStatus::Running.to_string(), "running");
        assert_eq!(TeamStatus::Completed.to_string(), "completed");
        assert_eq!(TeamStatus::Failed.to_string(), "failed");
        assert_eq!(TeamStatus::Aborted.to_string(), "aborted");
    }

    #[test]
    fn test_sub_agent_new() {
        let agent = SubAgent::new("test-agent", "Test prompt");
        assert_eq!(agent.id, "test-agent");
        assert_eq!(agent.prompt, "Test prompt");
        assert_eq!(agent.status, SubAgentStatus::Pending);
        assert!(agent.dependencies.is_empty());
    }

    #[test]
    fn test_sub_agent_lifecycle() {
        let mut agent = SubAgent::new("agent-1", "Do something");
        
        agent.start();
        assert_eq!(agent.status, SubAgentStatus::Running);
        
        agent.complete("Result text".to_string(), 100, 0.01);
        assert_eq!(agent.status, SubAgentStatus::Completed);
        assert_eq!(agent.result, Some("Result text".to_string()));
        assert_eq!(agent.tokens_used, 100);
        assert_eq!(agent.cost_usd, 0.01);
    }

    #[test]
    fn test_sub_agent_fail() {
        let mut agent = SubAgent::new("agent-1", "Do something");
        agent.fail("Something went wrong".to_string());
        assert_eq!(agent.status, SubAgentStatus::Failed);
        assert_eq!(agent.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_sub_agent_dependencies() {
        let agent = SubAgent::new("agent-2", "Dependent task")
            .with_dependency("agent-1");
        assert_eq!(agent.dependencies, vec!["agent-1"]);
    }

    #[test]
    fn test_sub_agent_is_ready() {
        let agent = SubAgent::new("agent-2", "Dependent task")
            .with_dependency("agent-1");
        
        // Not ready if dependency not completed
        let completed: std::collections::HashSet<&str> = std::collections::HashSet::new();
        assert!(!agent.is_ready(&completed));
        
        // Ready if dependency completed
        let mut completed: std::collections::HashSet<&str> = std::collections::HashSet::new();
        completed.insert("agent-1");
        assert!(agent.is_ready(&completed));
    }

    #[test]
    fn test_agent_team_new() {
        let team = AgentTeam::new("team-123", "Analyze codebase");
        assert_eq!(team.id, "team-123");
        assert_eq!(team.task, "Analyze codebase");
        assert_eq!(team.mode, TeamMode::Parallel);
        assert_eq!(team.status, TeamStatus::Pending);
        assert!(team.agents.is_empty());
    }

    #[test]
    fn test_agent_team_with_mode() {
        let team = AgentTeam::new("team-123", "Task")
            .with_mode(TeamMode::Sequence);
        assert_eq!(team.mode, TeamMode::Sequence);
    }

    #[test]
    fn test_agent_team_create_parallel_agents() {
        let mut team = AgentTeam::new("team-1", "Task");
        team.create_parallel_agents(3, "Analyze module");
        
        assert_eq!(team.agents.len(), 3);
        assert_eq!(team.agents[0].id, "agent-0");
        assert_eq!(team.agents[1].id, "agent-1");
        assert_eq!(team.agents[2].id, "agent-2");
    }

    #[test]
    fn test_agent_team_lifecycle() {
        let mut team = AgentTeam::new("team-1", "Task");
        
        team.start();
        assert_eq!(team.status, TeamStatus::Running);
        assert!(team.started_at.is_some());
        
        team.complete();
        assert_eq!(team.status, TeamStatus::Completed);
        assert!(team.completed_at.is_some());
    }

    #[test]
    fn test_agent_team_abort() {
        let mut team = AgentTeam::new("team-1", "Task");
        team.start();
        team.abort("User cancelled".to_string());
        
        assert_eq!(team.status, TeamStatus::Aborted);
    }

    #[test]
    fn test_agent_team_collect_results() {
        let mut team = AgentTeam::new("team-1", "Task");
        
        let mut agent1 = SubAgent::new("agent-1", "Task 1");
        agent1.complete("Result 1".to_string(), 100, 0.01);
        team.add_agent(agent1);
        
        let agent2 = SubAgent::new("agent-2", "Task 2");
        team.add_agent(agent2);
        
        let results = team.collect_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results.get("agent-1"), Some(&"Result 1".to_string()));
    }

    #[test]
    fn test_team_budget_exceeded() {
        let mut team = AgentTeam::new("team-1", "Task")
            .with_budget(TeamBudget {
                max_cost_usd: Some(0.1),
                max_tokens: None,
                max_time_secs: None,
            });
        
        let mut agent = SubAgent::new("agent-1", "Task");
        agent.complete("Result".to_string(), 100, 0.05);
        team.add_agent(agent);
        team.calculate_totals();
        
        assert!(!team.is_budget_exceeded());
        
        // Add another agent to exceed budget
        let mut agent2 = SubAgent::new("agent-2", "Task");
        agent2.complete("Result".to_string(), 100, 0.06);
        team.add_agent(agent2);
        team.calculate_totals();
        
        assert!(team.is_budget_exceeded());
    }

    #[test]
    fn test_team_summary() {
        let mut team = AgentTeam::new("team-1", "Task");
        
        let mut agent1 = SubAgent::new("agent-1", "Task 1");
        agent1.complete("Result".to_string(), 100, 0.01);
        team.add_agent(agent1);
        
        let mut agent2 = SubAgent::new("agent-2", "Task 2");
        agent2.fail("Error".to_string());
        team.add_agent(agent2);
        
        let agent3 = SubAgent::new("agent-3", "Task 3");
        team.add_agent(agent3);
        
        team.calculate_totals();
        let summary = team.summary();
        
        assert_eq!(summary.total_agents, 3);
        assert_eq!(summary.completed_count, 1);
        assert_eq!(summary.failed_count, 1);
        assert_eq!(summary.pending_count, 1);
        assert_eq!(summary.total_tokens, 100);
        assert_eq!(summary.total_cost_usd, 0.01);
    }

    #[test]
    fn test_shared_state() {
        let mut state = SharedState::new();
        
        state.set("key1", serde_json::json!("value1"));
        assert_eq!(state.get("key1"), Some(&serde_json::json!("value1")));
        
        state.report_progress("agent-1", "50% done", 0.5);
        assert_eq!(state.progress.len(), 1);
        assert_eq!(state.progress[0].agent_id, "agent-1");
        assert_eq!(state.progress[0].progress, 0.5);
    }

    #[test]
    fn test_team_json_serialization() {
        let team = AgentTeam::new("team-1", "Test task")
            .with_mode(TeamMode::Parallel)
            .with_name("Test Team");
        
        let json = serde_json::to_string(&team).expect("Failed to serialize");
        let deserialized: AgentTeam = serde_json::from_str(&json).expect("Failed to deserialize");
        
        assert_eq!(deserialized.id, "team-1");
        assert_eq!(deserialized.task, "Test task");
        assert_eq!(deserialized.mode, TeamMode::Parallel);
        assert_eq!(deserialized.name, Some("Test Team".to_string()));
    }

    #[test]
    fn test_sub_agent_json_serialization() {
        let agent = SubAgent::new("agent-1", "Test prompt")
            .with_dependency("agent-0");
        
        let json = serde_json::to_string(&agent).expect("Failed to serialize");
        let deserialized: SubAgent = serde_json::from_str(&json).expect("Failed to deserialize");
        
        assert_eq!(deserialized.id, "agent-1");
        assert_eq!(deserialized.prompt, "Test prompt");
        assert_eq!(deserialized.dependencies, vec!["agent-0"]);
    }
}
