//! Sub-Agent Teams / Parallel Execution
//!
//! This module provides functionality for creating and managing teams of
//! sub-agents that can execute tasks in parallel, sequence, or DAG mode.
//!
//! # Architecture
//!
//! - `types` - Core types: AgentTeam, SubAgent, TeamMode, TeamStatus
//! - `executor` - Team execution engine: TeamExecutor, TeamResult
//! - `policy` - Tool permissions, budget control, and result collection
//! - `agent` - Sub-agent execution with policy enforcement
//! - `shared_state` - Inter-agent communication: key-value store, progress broadcast, result merging
//!
//! # Execution Modes
//!
//! - **Parallel**: All agents execute simultaneously
//! - **Sequence**: Agents execute one after another
//! - **DAG**: Agents execute according to dependency graph
//!
//! # Inter-Agent Communication
//!
//! Agents can communicate through the shared state:
//!
//! - **Key-Value Store**: Thread-safe shared data storage
//! - **Progress Broadcast**: Real-time progress updates to subscribers
//! - **Result Merging**: Combine results from multiple agents
//!
//! # Configuration
//!
//! Teams can be created via CLI commands:
//!
//! ```text
//! /team spawn 3 --parallel "Analyze the codebase"
//! /team spawn --sequence "First analyze, then implement"
//! /team spawn --dag "Check code style and tests, then generate report"
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use baoclaw_core::engine::team::{TeamExecutor, AgentTeam, TeamMode, TeamPolicy, SharedStateManager};
//!
//! // Create an executor
//! let executor = TeamExecutor::new(api_client, tools, cwd, model);
//!
//! // Create a team for parallel execution with policy
//! let policy = TeamPolicy::safe_tools()
//!     .with_total_budget(2.0)
//!     .with_max_turns_per_agent(10);
//!
//! let mut team = executor.create_team("Analyze codebase".to_string(), TeamConfig {
//!     mode: TeamMode::Parallel,
//!     policy: Some(policy),
//!     ..Default::default()
//! }).await?;
//!
//! // Add agents
//! executor.add_parallel_agents(&mut team, 3, "Analyze module").await?;
//!
//! // Execute the team
//! let result = executor.execute(team).await;
//!
//! // Use shared state for inter-agent communication
//! let shared_state = SharedStateManager::new();
//! shared_state.set("key", serde_json::json!("value")).await;
//! let mut progress_rx = shared_state.subscribe_progress();
//! ```

pub mod agent;
pub mod executor;
pub mod manager;
pub mod policy;
pub mod scheduler;
pub mod shared_state;
pub mod types;

// Re-export main types for convenience
pub use agent::{BudgetEnforcer, ExecutionTracker, SubAgentError, SubAgentExecutor};
pub use executor::{TeamConfig, TeamError, TeamExecutor, TeamResult};
pub use manager::TeamManager;
pub use policy::{
    AgentPolicy,
    AgentResult,
    AgentUsage,
    BudgetExceededAction,
    DepthTools,
    TeamPolicy,
    TeamResults,
};
pub use shared_state::{
    AgentResultForMerge,
    MergedResults,
    MergeStrategy,
    ProgressEvent,
    SharedMetrics as SharedManagerMetrics,
    SharedStateManager,
};
pub use scheduler::{
    DagNode, DagScheduler, DagStats, ExecutionWave, NodeStatus, SchedulerError,
};
pub use types::{
    AgentTeam,
    ProgressReport,
    SharedState,
    SubAgent,
    SubAgentStatus,
    TeamBudget,
    TeamMode,
    TeamStatus,
    TeamSummary,
};
