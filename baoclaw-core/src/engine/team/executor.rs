//! Team Executor for executing teams of sub-agents.
//!
//! This module provides the `TeamExecutor` struct which orchestrates the execution
//! of teams of sub-agents in parallel, sequence, or DAG mode.
//!
//! # Execution Modes
//!
//! - **Parallel**: All agents execute simultaneously, results are collected at the end.
//! - **Sequence**: Agents execute one after another, each receiving the previous result.
//! - **DAG**: Agents execute according to a dependency graph, with parallel execution
//!   of independent nodes.
//!
//! # Example
//!
//! ```rust,ignore
//! use baoclaw_core::engine::team::{TeamExecutor, AgentTeam, TeamMode};
//!
//! let executor = TeamExecutor::new(api_client, tools);
//! let team = AgentTeam::new("team-1", "Analyze codebase")
//!     .with_mode(TeamMode::Parallel);
//!
//! let result = executor.execute(team).await;
//! ```

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};
use tokio::task::JoinSet;

use crate::api::unified::UnifiedClient;
use crate::engine::query_engine::{EngineEvent, QueryEngine, QueryEngineConfig, ThinkingConfig};
use crate::engine::team::types::{
    AgentTeam, SubAgent, SubAgentStatus, TeamBudget, TeamMode, TeamStatus,
};
use crate::tools::trait_def::Tool;
use serde::{Deserialize, Serialize};

/// Error type for team execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

impl std::fmt::Display for TeamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref agent_id) = self.agent_id {
            write!(f, "[{}] {}: {}", self.code, agent_id, self.message)
        } else {
            write!(f, "[{}]: {}", self.code, self.message)
        }
    }
}

impl std::error::Error for TeamError {}

/// Result of a team execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamResult {
    /// The team that was executed.
    pub team: AgentTeam,
    /// Whether execution was successful.
    pub success: bool,
    /// Error message if execution failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Duration of execution in milliseconds.
    pub duration_ms: u64,
}

/// Configuration for creating a new team.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamConfig {
    /// Human-readable name for the team.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Execution mode.
    #[serde(default)]
    pub mode: TeamMode,
    /// Budget constraints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<TeamBudget>,
    /// Working directory for the team.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Model to use for sub-agents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Maximum turns per sub-agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    /// Execution policy for tool permissions and budget control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<crate::engine::team::policy::TeamPolicy>,
}

impl Default for TeamConfig {
    fn default() -> Self {
        Self {
            name: None,
            mode: TeamMode::Parallel,
            budget: None,
            cwd: None,
            model: None,
            max_turns: Some(10),
            policy: None,
        }
    }
}

/// Manages teams of sub-agents and their execution.
pub struct TeamExecutor {
    /// API client for creating sub-agent engines.
    api_client: Arc<UnifiedClient>,
    /// Tools available to sub-agents.
    tools: Vec<Arc<dyn Tool>>,
    /// Active teams being managed.
    teams: Arc<RwLock<HashMap<String, AgentTeam>>>,
    /// Abort handles for running teams.
    abort_handles: Arc<RwLock<HashMap<String, watch::Sender<bool>>>>,
    /// Default working directory.
    default_cwd: PathBuf,
    /// Default model.
    default_model: String,
    /// Default policy for team execution.
    default_policy: crate::engine::team::policy::TeamPolicy,
}

impl TeamExecutor {
    /// Create a new TeamExecutor.
    pub fn new(
        api_client: Arc<UnifiedClient>,
        tools: Vec<Arc<dyn Tool>>,
        default_cwd: PathBuf,
        default_model: String,
    ) -> Self {
        Self {
            api_client,
            tools,
            teams: Arc::new(RwLock::new(HashMap::new())),
            abort_handles: Arc::new(RwLock::new(HashMap::new())),
            default_cwd,
            default_model,
            default_policy: crate::engine::team::policy::TeamPolicy::default(),
        }
    }

    /// Create a TeamExecutor with a custom default policy.
    pub fn with_policy(
        api_client: Arc<UnifiedClient>,
        tools: Vec<Arc<dyn Tool>>,
        default_cwd: PathBuf,
        default_model: String,
        default_policy: crate::engine::team::policy::TeamPolicy,
    ) -> Self {
        Self {
            api_client,
            tools,
            teams: Arc::new(RwLock::new(HashMap::new())),
            abort_handles: Arc::new(RwLock::new(HashMap::new())),
            default_cwd,
            default_model,
            default_policy,
        }
    }

    /// Create a new team with the given task and configuration.
    ///
    /// This creates the team structure but does not start execution.
    /// Use `execute()` to start execution.
    pub async fn create_team(
        &self,
        task: String,
        config: TeamConfig,
    ) -> Result<AgentTeam, TeamError> {
        let team_id = uuid::Uuid::new_v4().to_string()[..8].to_string();

        let mut team = AgentTeam::new(team_id.clone(), task);

        if let Some(name) = config.name {
            team = team.with_name(name);
        }

        team.mode = config.mode;

        if let Some(budget) = config.budget {
            team.budget = Some(budget);
        }

        if let Some(cwd) = config.cwd {
            team.cwd = Some(cwd);
        } else {
            team.cwd = Some(self.default_cwd.to_string_lossy().to_string());
        }

        // Store policy in team metadata
        if let Some(policy) = config.policy {
            team.shared_state.set(
                "policy",
                serde_json::to_value(&policy).unwrap_or(serde_json::json!(null)),
            );
        } else {
            // Use default policy
            team.shared_state.set(
                "policy",
                serde_json::to_value(&self.default_policy).unwrap_or(serde_json::json!(null)),
            );
        }

        // Store max_turns if specified
        if let Some(max_turns) = config.max_turns {
            team.shared_state.set("max_turns", serde_json::json!(max_turns));
        }

        // Store the team
        self.teams.write().await.insert(team_id, team.clone());

        Ok(team)
    }

    /// Add sub-agents to a team for parallel execution.
    ///
    /// Creates `count` agents with the same prompt prefix.
    pub async fn add_parallel_agents(
        &self,
        team: &mut AgentTeam,
        count: usize,
        prompt_prefix: &str,
    ) -> Result<(), TeamError> {
        if team.status != TeamStatus::Pending {
            return Err(TeamError {
                code: "invalid_state".to_string(),
                message: format!("Team is already {} and cannot accept new agents", team.status),
                agent_id: None,
            });
        }

        team.create_parallel_agents(count, prompt_prefix);

        // Update stored team
        self.teams
            .write()
            .await
            .insert(team.id.clone(), team.clone());

        Ok(())
    }

    /// Add a sub-agent to a team.
    pub async fn add_agent(
        &self,
        team: &mut AgentTeam,
        agent: SubAgent,
    ) -> Result<(), TeamError> {
        if team.status != TeamStatus::Pending {
            return Err(TeamError {
                code: "invalid_state".to_string(),
                message: format!("Team is already {} and cannot accept new agents", team.status),
                agent_id: None,
            });
        }

        team.add_agent(agent);

        // Update stored team
        self.teams
            .write()
            .await
            .insert(team.id.clone(), team.clone());

        Ok(())
    }

    /// Execute a team.
    ///
    /// The execution mode (parallel, sequence, dag) is determined by `team.mode`.
    pub async fn execute(&self, mut team: AgentTeam) -> TeamResult {
        let start_time = std::time::Instant::now();

        // Validate team has agents
        if team.agents.is_empty() {
            return TeamResult {
                team,
                success: false,
                error: Some("Team has no agents to execute".to_string()),
                duration_ms: start_time.elapsed().as_millis() as u64,
            };
        }

        // Check if team can be started
        if team.status != TeamStatus::Pending {
            let status = team.status.clone();
            return TeamResult {
                team,
                success: false,
                error: Some(format!("Team is already {}", status)),
                duration_ms: start_time.elapsed().as_millis() as u64,
            };
        }

        // Create abort handle
        let (abort_tx, abort_rx) = watch::channel(false);
        self.abort_handles
            .write()
            .await
            .insert(team.id.clone(), abort_tx);

        // Start the team
        team.start();
        self.teams
            .write()
            .await
            .insert(team.id.clone(), team.clone());

        // Execute based on mode
        let result = match team.mode {
            TeamMode::Parallel => self.execute_parallel(team, abort_rx).await,
            TeamMode::Sequence => self.execute_sequence(team, abort_rx).await,
            TeamMode::Dag => self.execute_dag(team, abort_rx).await,
        };

        // Clean up abort handle
        self.abort_handles.write().await.remove(&result.team.id);

        let duration_ms = start_time.elapsed().as_millis() as u64;
        TeamResult {
            team: result.team,
            success: result.success,
            error: result.error,
            duration_ms,
        }
    }

    /// Execute all agents in parallel.
    ///
    /// All agents start simultaneously and execute independently.
    /// Results are collected as agents complete.
    async fn execute_parallel(
        &self,
        mut team: AgentTeam,
        abort_rx: watch::Receiver<bool>,
    ) -> TeamResult {
        use crate::engine::team::policy::AgentResult;

        let shared_state = Arc::new(RwLock::new(team.shared_state.clone()));
        let cwd = team
            .cwd
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_cwd.clone());

        // Get team policy
        let team_policy = self.get_team_policy(&team);

        // Collect agent prompts for parallel execution
        let agent_prompts: Vec<(String, String)> = team
            .agents
            .iter()
            .map(|a| (a.id.clone(), a.prompt.clone()))
            .collect();

        // Use JoinSet for concurrent execution
        let mut join_set = JoinSet::new();

        for (agent_id, prompt) in agent_prompts {
            let api_client = Arc::clone(&self.api_client);
            let tools = self.tools.clone();
            let cwd_clone = cwd.clone();
            let model = self.default_model.clone();
            let abort_rx_clone = abort_rx.clone();
            let agent_id_clone = agent_id.clone();
            let agent_policy =
                crate::engine::team::policy::AgentPolicy::from_team_policy(&team_policy, 1);

            join_set.spawn(async move {
                let result = Self::execute_single_agent(
                    api_client,
                    tools,
                    cwd_clone,
                    model,
                    prompt,
                    abort_rx_clone,
                    Some(agent_policy),
                    agent_id_clone.clone(),
                )
                .await;

                (agent_id_clone, result)
            });
        }

        // Collect results
        let mut any_failed = false;
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((agent_id, agent_result)) => {
                    if let Some(agent) = team.get_agent_mut(&agent_id) {
                        match agent_result {
                            Ok(result) => {
                                // result is AgentResult
                                if result.success {
                                    agent.complete(
                                        result.text.clone().unwrap_or_default(),
                                        result.usage.as_ref().map(|u| u.total()).unwrap_or(0),
                                        result.cost_usd,
                                    );
                                } else {
                                    agent.fail(result.error.clone().unwrap_or_default());
                                    any_failed = true;
                                }
                            }
                            Err(e) => {
                                agent.fail(e.message.clone());
                                any_failed = true;
                            }
                        }
                    }

                    // Update shared state
                    shared_state.write().await.set(
                        format!("agent_{}_complete", agent_id),
                        serde_json::json!(true),
                    );
                }
                Err(_e) => {
                    // Task panicked
                    any_failed = true;
                }
            }
        }

        // Finalize team
        team.shared_state = shared_state.read().await.clone();
        team.calculate_totals();

        if any_failed {
            team.fail("One or more agents failed");
        } else {
            team.complete();
        }

        // Update stored team
        self.teams
            .write()
            .await
            .insert(team.id.clone(), team.clone());

        TeamResult {
            team,
            success: !any_failed,
            error: if any_failed {
                Some("One or more agents failed".to_string())
            } else {
                None
            },
            duration_ms: 0, // Set by caller
        }
    }

    /// Execute agents in sequence.
    ///
    /// Each agent waits for the previous one to complete.
    /// The result of each agent is passed to the next via shared state.
    async fn execute_sequence(
        &self,
        mut team: AgentTeam,
        abort_rx: watch::Receiver<bool>,
    ) -> TeamResult {
        let shared_state = Arc::new(RwLock::new(team.shared_state.clone()));
        let cwd = team
            .cwd
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_cwd.clone());

        // Get team policy
        let team_policy = self.get_team_policy(&team);

        let mut previous_result: Option<String> = None;
        let mut any_failed = false;

        for agent in &mut team.agents {
            // Check for abort
            if *abort_rx.borrow() {
                agent.skip("Team aborted".to_string());
                any_failed = true;
                continue;
            }

            // Build prompt with context from previous agent
            let prompt = if let Some(ref prev) = previous_result {
                format!(
                    "{}\n\nPrevious agent result:\n{}\n\nContinue from where the previous agent left off.",
                    agent.prompt, prev
                )
            } else {
                agent.prompt.clone()
            };

            // Create agent policy
            let agent_policy =
                crate::engine::team::policy::AgentPolicy::from_team_policy(&team_policy, 1);

            // Execute the agent
            let result = Self::execute_single_agent(
                Arc::clone(&self.api_client),
                self.tools.clone(),
                cwd.clone(),
                self.default_model.clone(),
                prompt,
                abort_rx.clone(),
                Some(agent_policy),
                agent.id.clone(),
            )
            .await;

            match result {
                Ok(agent_result) => {
                    if agent_result.success {
                        let text = agent_result.text.clone().unwrap_or_default();
                        agent.complete(
                            text.clone(),
                            agent_result.usage.as_ref().map(|u| u.total()).unwrap_or(0),
                            agent_result.cost_usd,
                        );
                        previous_result = Some(text.clone());

                        // Store result in shared state
                        shared_state
                            .write()
                            .await
                            .set(format!("agent_{}_result", agent.id), serde_json::json!(text));
                    } else {
                        agent.fail(agent_result.error.clone().unwrap_or_default());
                        any_failed = true;
                        previous_result = None;
                    }
                }
                Err(e) => {
                    agent.fail(e.message.clone());
                    any_failed = true;
                    previous_result = None;
                }
            }
        }

        // Finalize team
        team.shared_state = shared_state.read().await.clone();
        team.calculate_totals();

        if any_failed {
            team.fail("One or more agents failed in sequence");
        } else {
            team.complete();
        }

        // Update stored team
        self.teams
            .write()
            .await
            .insert(team.id.clone(), team.clone());

        TeamResult {
            team,
            success: !any_failed,
            error: if any_failed {
                Some("One or more agents failed in sequence".to_string())
            } else {
                None
            },
            duration_ms: 0,
        }
    }

    /// Execute agents according to a DAG using the DagScheduler.
    ///
    /// Agents with no dependencies start first.
    /// As agents complete, their dependents become ready and execute.
    /// The DagScheduler handles topological sorting, dependency resolution,
    /// cycle detection, and parallel execution of ready nodes.
    async fn execute_dag(
        &self,
        mut team: AgentTeam,
        abort_rx: watch::Receiver<bool>,
    ) -> TeamResult {
        use crate::engine::team::scheduler::{DagScheduler, NodeStatus};

        let shared_state = Arc::new(RwLock::new(team.shared_state.clone()));
        let cwd = team
            .cwd
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_cwd.clone());

        // Get team policy
        let team_policy = self.get_team_policy(&team);

        // Create the DAG scheduler from the team
        let mut scheduler = match DagScheduler::from_team(&team) {
            Ok(s) => s,
            Err(e) => {
                return TeamResult {
                    team,
                    success: false,
                    error: Some(format!("Failed to create DAG scheduler: {}", e.message)),
                    duration_ms: 0,
                };
            }
        };

        // Build and validate the DAG (detects cycles, validates dependencies)
        if let Err(e) = scheduler.build() {
            return TeamResult {
                team,
                success: false,
                error: Some(format!("Invalid DAG structure: {}", e.message)),
                duration_ms: 0,
            };
        }

        // Track running tasks
        let mut join_set = JoinSet::new();
        let mut any_failed = false;

        // Process until scheduler is complete
        while !scheduler.is_complete() {
            // Check for abort
            if *abort_rx.borrow() {
                // Mark all pending agents as skipped
                for agent in &mut team.agents {
                    if agent.status == SubAgentStatus::Pending {
                        agent.skip("Team aborted".to_string());
                        if let Err(_) = scheduler.fail_node(&agent.id, Some("Team aborted")) {
                            // Ignore errors during abort
                        }
                    }
                }
                any_failed = true;
                break;
            }

            // Get completed agent IDs
            let completed_ids: HashSet<String> = team
                .agents
                .iter()
                .filter(|a| a.status == SubAgentStatus::Completed)
                .map(|a| a.id.clone())
                .collect();

            // Get running count
            let running_count = team
                .agents
                .iter()
                .filter(|a| a.status == SubAgentStatus::Running)
                .count();

            // Get ready nodes from scheduler (respects max parallelism)
            let ready_nodes = scheduler.get_ready_nodes(
                &completed_ids.iter().map(|s| s.as_str()).collect(),
                running_count,
            );

            // Start ready agents
            for agent_id in ready_nodes {
                // Get agent prompt
                let prompt = team
                    .get_agent(&agent_id)
                    .map(|a| a.prompt.clone())
                    .unwrap_or_default();

                // Mark as running in both scheduler and team
                if let Err(_) = scheduler.start_node(&agent_id) {
                    continue;
                }
                if let Some(agent) = team.get_agent_mut(&agent_id) {
                    agent.start();
                }

                let api_client = Arc::clone(&self.api_client);
                let tools = self.tools.clone();
                let cwd_clone = cwd.clone();
                let model = self.default_model.clone();
                let shared_state_clone = Arc::clone(&shared_state);
                let agent_policy = crate::engine::team::policy::AgentPolicy::from_team_policy(
                    &team_policy,
                    1,
                );
                let agent_id_for_result = agent_id.clone();

                join_set.spawn(async move {
                    let result = Self::execute_single_agent(
                        api_client,
                        tools,
                        cwd_clone,
                        model,
                        prompt,
                        watch::channel(false).1, // No abort for individual agent
                        Some(agent_policy),
                        agent_id_for_result.clone(),
                    )
                    .await;

                    (agent_id_for_result, result, shared_state_clone)
                });
            }

            // Wait for at least one agent to complete
            if join_set.is_empty() && !scheduler.is_complete() {
                // No running tasks but not complete - this means we have a cycle
                // that wasn't detected (shouldn't happen, but handle gracefully)
                any_failed = true;
                break;
            }

            if !join_set.is_empty() {
                if let Some(result) = join_set.join_next().await {
                    match result {
                        Ok((agent_id, agent_result, state)) => {
                            match agent_result {
                                Ok(result) => {
                                    if result.success {
                                        // Mark as completed in scheduler
                                        let _ = scheduler.complete_node(&agent_id);

                                        // Update agent status
                                        if let Some(agent) = team.get_agent_mut(&agent_id) {
                                            agent.complete(
                                                result.text.clone().unwrap_or_default(),
                                                result.usage.as_ref().map(|u| u.total()).unwrap_or(0),
                                                result.cost_usd,
                                            );
                                        }

                                        // Store result in shared state
                                        if let Some(text) = &result.text {
                                            state.write().await.set(
                                                format!("agent_{}_result", agent_id),
                                                serde_json::json!(text),
                                            );
                                        }
                                    } else {
                                        // Mark as failed in scheduler
                                        let _ = scheduler.fail_node(
                                            &agent_id,
                                            result.error.as_deref(),
                                        );

                                        // Skip all dependents
                                        let _ = scheduler.skip_dependents(
                                            &agent_id,
                                            "Dependency failed",
                                        );

                                        // Update agent status
                                        if let Some(agent) = team.get_agent_mut(&agent_id) {
                                            agent.fail(result.error.clone().unwrap_or_default());
                                        }

                                        // Update skipped agents in team
                                        for team_agent in &mut team.agents {
                                            if let Some(node) = scheduler.get_node(&team_agent.id) {
                                                if node.status == NodeStatus::Skipped {
                                                    team_agent.skip("Dependency failed".to_string());
                                                }
                                            }
                                        }

                                        any_failed = true;
                                    }
                                }
                                Err(e) => {
                                    // Mark as failed in scheduler
                                    let _ = scheduler.fail_node(&agent_id, Some(&e.message));

                                    // Skip all dependents
                                    let _ = scheduler.skip_dependents(
                                        &agent_id,
                                        "Dependency failed",
                                    );

                                    // Update agent status
                                    if let Some(agent) = team.get_agent_mut(&agent_id) {
                                        agent.fail(e.message.clone());
                                    }

                                    // Update skipped agents in team
                                    for team_agent in &mut team.agents {
                                        if let Some(node) = scheduler.get_node(&team_agent.id) {
                                            if node.status == NodeStatus::Skipped {
                                                team_agent.skip("Dependency failed".to_string());
                                            }
                                        }
                                    }

                                    any_failed = true;
                                }
                            }
                        }
                        Err(_e) => {
                            any_failed = true;
                        }
                    }
                }
            }
        }

        // Finalize team
        team.shared_state = shared_state.read().await.clone();
        team.calculate_totals();

        if any_failed {
            team.fail("One or more agents failed in DAG execution");
        } else {
            team.complete();
        }

        // Update stored team
        self.teams
            .write()
            .await
            .insert(team.id.clone(), team.clone());

        TeamResult {
            team,
            success: !any_failed,
            error: if any_failed {
                Some("One or more agents failed in DAG execution".to_string())
            } else {
                None
            },
            duration_ms: 0,
        }
    }

    /// Execute a single sub-agent.
    ///
    /// Creates a QueryEngine and runs the prompt, returning the result.
    /// Applies policy restrictions on tools, budget, and turns.
    async fn execute_single_agent(
        api_client: Arc<UnifiedClient>,
        tools: Vec<Arc<dyn Tool>>,
        cwd: PathBuf,
        model: String,
        prompt: String,
        mut abort_rx: watch::Receiver<bool>,
        agent_policy: Option<crate::engine::team::policy::AgentPolicy>,
        agent_id: String,
    ) -> Result<crate::engine::team::policy::AgentResult, TeamError> {
        use crate::engine::team::policy::{AgentPolicy, AgentResult, AgentUsage};

        let start_time = std::time::Instant::now();

        // Get policy constraints or use defaults
        let max_turns = agent_policy
            .as_ref()
            .map(|p| p.max_turns)
            .unwrap_or(10);
        let max_budget = agent_policy
            .as_ref()
            .and_then(|p| p.max_cost_usd);

        // Filter tools based on policy
        let filtered_tools = if let Some(ref policy) = agent_policy {
            let tool_names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
            let allowed_names: HashSet<String> = tool_names
                .iter()
                .filter(|name| policy.is_tool_allowed(name))
                .map(|s| s.to_string())
                .collect();
            tools
                .into_iter()
                .filter(|t| allowed_names.contains(t.name()))
                .collect()
        } else {
            tools
        };

        let config = QueryEngineConfig {
            cwd,
            tools: filtered_tools,
            api_client,
            model,
            thinking_config: ThinkingConfig::Disabled,
            max_turns: Some(max_turns),
            max_budget_usd: max_budget,
            verbose: false,
            custom_system_prompt: Some(
                "You are a sub-agent. Complete the given task efficiently and report results."
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

        let mut engine = QueryEngine::new(config);
        let mut rx = engine.submit_message(prompt).await;

        let mut final_text = String::new();
        let mut total_input_tokens: u64 = 0;
        let mut total_output_tokens: u64 = 0;
        let mut total_cost: f64 = 0.0;
        let mut turns: u32 = 0;
        let mut tools_used: Vec<String> = Vec::new();
        let mut files_read: Vec<String> = Vec::new();
        let mut files_written: Vec<String> = Vec::new();
        let mut commands_executed: Vec<String> = Vec::new();

        loop {
            tokio::select! {
                // Check for abort
                _ = abort_rx.changed() => {
                    if *abort_rx.borrow() {
                        engine.abort();
                        return Err(TeamError {
                            code: "aborted".to_string(),
                            message: "Agent execution was aborted".to_string(),
                            agent_id: Some(agent_id),
                        });
                    }
                }

                // Process events
                event = rx.recv() => {
                    match event {
                        Some(EngineEvent::AssistantChunk { content, .. }) => {
                            final_text.push_str(&content);
                        }
                        Some(EngineEvent::Result(result)) => {
                            if let Some(text) = result.text {
                                final_text = text;
                            }
                            total_input_tokens += result.usage.input_tokens;
                            total_output_tokens += result.usage.output_tokens;
                            total_cost += result.total_cost_usd;
                            // turns tracking is not available in QueryResult
                            break;
                        }
                        Some(EngineEvent::Error(err)) => {
                            return Err(TeamError {
                                code: err.code,
                                message: err.message,
                                agent_id: Some(agent_id),
                            });
                        }
                        None => break,
                        _ => {}
                    }
                }
            }
        }

        // Build result
        let usage = AgentUsage::new(total_input_tokens, total_output_tokens);
        let duration_ms = start_time.elapsed().as_millis() as u64;

        let result = AgentResult::success(agent_id.clone(), final_text)
            .with_usage(usage)
            .with_cost(total_cost)
            .with_duration(duration_ms)
            .with_turns(turns);

        // Note: In a real implementation, we would track tools_used, files_read, files_written
        // by observing tool execution events from the engine. For now, we return the basic result.

        Ok(result)
    }

    /// Get the status of a team.
    pub async fn get_team_status(&self, team_id: &str) -> Option<AgentTeam> {
        self.teams.read().await.get(team_id).cloned()
    }

    /// List all teams.
    pub async fn list_teams(&self) -> Vec<AgentTeam> {
        self.teams.read().await.values().cloned().collect()
    }

    /// Abort a running team.
    pub async fn abort_team(&self, team_id: &str) -> bool {
        // Send abort signal
        if let Some(tx) = self.abort_handles.read().await.get(team_id) {
            let _ = tx.send(true);
        }

        // Update team status
        if let Some(team) = self.teams.write().await.get_mut(team_id) {
            if team.status == TeamStatus::Running {
                team.abort("User requested abort");
                return true;
            }
        }

        false
    }

    /// Collect results from all completed agents in a team.
    pub async fn collect_results(&self, team_id: &str) -> Option<HashMap<String, String>> {
        self.teams
            .read()
            .await
            .get(team_id)
            .map(|team| team.collect_results())
    }

    /// Collect structured results from a team execution.
    ///
    /// Returns a TeamResults object with comprehensive metadata including
    /// total usage, cost, duration, and per-agent results.
    pub async fn collect_team_results(
        &self,
        team_id: &str,
    ) -> Option<crate::engine::team::policy::TeamResults> {
        use crate::engine::team::policy::{AgentResult, TeamResults};

        let team = self.teams.read().await.get(team_id).cloned()?;

        let mut results = TeamResults::with_team_id(team_id);

        for agent in &team.agents {
            let agent_result = match agent.status {
                SubAgentStatus::Completed => {
                    let mut result = AgentResult::success(
                        agent.id.clone(),
                        agent.result.clone().unwrap_or_default(),
                    );
                    result.cost_usd = agent.cost_usd;
                    result.cost_usd = agent.cost_usd;
                    result
                }
                SubAgentStatus::Failed => {
                    AgentResult::failure(agent.id.clone(), agent.error.clone().unwrap_or_default())
                }
                SubAgentStatus::Skipped => {
                    let mut result =
                        AgentResult::failure(agent.id.clone(), "Agent was skipped".to_string());
                    result.metadata.insert(
                        "skip_reason".to_string(),
                        serde_json::json!(
                            agent.metadata.get("skip_reason").unwrap_or(&"Unknown".to_string())
                        ),
                    );
                    result
                }
                _ => continue, // Skip pending/running agents
            };
            results.add_result(agent_result);
        }

        results.build_combined_text();
        Some(results)
    }

    /// Get the policy for a team.
    pub fn get_team_policy(
        &self,
        team: &AgentTeam,
    ) -> crate::engine::team::policy::TeamPolicy {
        use crate::engine::team::policy::TeamPolicy;

        team.shared_state
            .get("policy")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_else(|| self.default_policy.clone())
    }

    /// Create an agent policy from the team policy.
    pub fn create_agent_policy(
        &self,
        team: &AgentTeam,
        depth: u32,
    ) -> crate::engine::team::policy::AgentPolicy {
        use crate::engine::team::policy::AgentPolicy;

        let team_policy = self.get_team_policy(team);
        AgentPolicy::from_team_policy(&team_policy, depth)
    }

    /// Remove a completed team from the manager.
    pub async fn remove_team(&self, team_id: &str) -> Option<AgentTeam> {
        self.teams.write().await.remove(team_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::ApiClientConfig;

    fn make_api_client() -> Arc<UnifiedClient> {
        Arc::new(UnifiedClient::new_anthropic(ApiClientConfig {
            api_key: "test-key".to_string(),
            base_url: None,
            max_retries: None,
            api_path: None,
        }))
    }

    fn make_executor() -> TeamExecutor {
        let api_client = make_api_client();
        let tools: Vec<Arc<dyn Tool>> = vec![];
        TeamExecutor::new(
            api_client,
            tools,
            PathBuf::from("/tmp"),
            "claude-sonnet-4-20250514".to_string(),
        )
    }

    #[tokio::test]
    async fn test_create_team() {
        let executor = make_executor();

        let team = executor
            .create_team("Analyze codebase".to_string(), TeamConfig::default())
            .await
            .unwrap();

        assert_eq!(team.status, TeamStatus::Pending);
        assert_eq!(team.mode, TeamMode::Parallel);
        assert!(team.agents.is_empty());
    }

    #[tokio::test]
    async fn test_create_team_with_config() {
        let executor = make_executor();

        let config = TeamConfig {
            name: Some("Test Team".to_string()),
            mode: TeamMode::Sequence,
            budget: Some(TeamBudget {
                max_cost_usd: Some(1.0),
                max_tokens: Some(10000),
                max_time_secs: Some(60),
            }),
            cwd: Some("/home/user".to_string()),
            model: Some("claude-opus-4-20250514".to_string()),
            max_turns: Some(5),
            policy: None,
        };

        let team = executor
            .create_team("Complex task".to_string(), config)
            .await
            .unwrap();

        assert_eq!(team.name, Some("Test Team".to_string()));
        assert_eq!(team.mode, TeamMode::Sequence);
        assert!(team.budget.is_some());
        assert_eq!(team.cwd, Some("/home/user".to_string()));
    }

    #[tokio::test]
    async fn test_add_parallel_agents() {
        let executor = make_executor();

        let mut team = executor
            .create_team("Task".to_string(), TeamConfig::default())
            .await
            .unwrap();

        executor
            .add_parallel_agents(&mut team, 3, "Analyze module")
            .await
            .unwrap();

        assert_eq!(team.agents.len(), 3);
        assert_eq!(team.agents[0].id, "agent-0");
        assert_eq!(team.agents[1].id, "agent-1");
        assert_eq!(team.agents[2].id, "agent-2");
    }

    #[tokio::test]
    async fn test_add_agent() {
        let executor = make_executor();

        let mut team = executor
            .create_team("Task".to_string(), TeamConfig::default())
            .await
            .unwrap();

        let agent = SubAgent::new("custom-agent", "Do something specific");
        executor.add_agent(&mut team, agent).await.unwrap();

        assert_eq!(team.agents.len(), 1);
        assert_eq!(team.agents[0].id, "custom-agent");
    }

    #[tokio::test]
    async fn test_add_agent_to_running_team_fails() {
        let executor = make_executor();

        let mut team = executor
            .create_team("Task".to_string(), TeamConfig::default())
            .await
            .unwrap();

        team.start();

        let agent = SubAgent::new("agent-1", "Task");
        let result = executor.add_agent(&mut team, agent).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "invalid_state");
    }

    #[tokio::test]
    async fn test_get_team_status() {
        let executor = make_executor();

        let team = executor
            .create_team("Task".to_string(), TeamConfig::default())
            .await
            .unwrap();
        let team_id = team.id.clone();

        let retrieved = executor.get_team_status(&team_id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, team_id);
    }

    #[tokio::test]
    async fn test_get_team_status_not_found() {
        let executor = make_executor();

        let retrieved = executor.get_team_status("nonexistent").await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_list_teams() {
        let executor = make_executor();

        let team1 = executor
            .create_team("Task 1".to_string(), TeamConfig::default())
            .await
            .unwrap();
        let team2 = executor
            .create_team("Task 2".to_string(), TeamConfig::default())
            .await
            .unwrap();

        let teams = executor.list_teams().await;
        assert_eq!(teams.len(), 2);
    }

    #[tokio::test]
    async fn test_remove_team() {
        let executor = make_executor();

        let team = executor
            .create_team("Task".to_string(), TeamConfig::default())
            .await
            .unwrap();
        let team_id = team.id.clone();

        let removed = executor.remove_team(&team_id).await;
        assert!(removed.is_some());

        let retrieved = executor.get_team_status(&team_id).await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_execute_empty_team_fails() {
        let executor = make_executor();

        let team = executor
            .create_team("Task".to_string(), TeamConfig::default())
            .await
            .unwrap();

        let result = executor.execute(team).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("no agents"));
    }

    #[tokio::test]
    async fn test_execute_already_running_team_fails() {
        let executor = make_executor();

        let mut team = executor
            .create_team("Task".to_string(), TeamConfig::default())
            .await
            .unwrap();

        let agent = SubAgent::new("agent-1", "Task");
        team.add_agent(agent);
        team.start();

        let result = executor.execute(team).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("already"));
    }

    #[tokio::test]
    async fn test_team_result_serialization() {
        let team = AgentTeam::new("team-1", "Task");
        let result = TeamResult {
            team,
            success: true,
            error: None,
            duration_ms: 1000,
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: TeamResult = serde_json::from_str(&json).unwrap();

        assert!(deserialized.success);
        assert_eq!(deserialized.duration_ms, 1000);
    }

    #[tokio::test]
    async fn test_team_error_display() {
        let error = TeamError {
            code: "test_error".to_string(),
            message: "Something went wrong".to_string(),
            agent_id: Some("agent-1".to_string()),
        };

        let display = format!("{}", error);
        assert!(display.contains("test_error"));
        assert!(display.contains("agent-1"));
        assert!(display.contains("Something went wrong"));
    }

    #[tokio::test]
    async fn test_team_config_default() {
        let config = TeamConfig::default();

        assert!(config.name.is_none());
        assert_eq!(config.mode, TeamMode::Parallel);
        assert!(config.budget.is_none());
        assert!(config.cwd.is_none());
        assert!(config.model.is_none());
        assert_eq!(config.max_turns, Some(10));
    }

    #[tokio::test]
    async fn test_abort_team() {
        let executor = make_executor();

        let team = executor
            .create_team("Task".to_string(), TeamConfig::default())
            .await
            .unwrap();
        let team_id = team.id.clone();

        // Team is pending, not running, so abort should return false
        let aborted = executor.abort_team(&team_id).await;
        assert!(!aborted);
    }
}
