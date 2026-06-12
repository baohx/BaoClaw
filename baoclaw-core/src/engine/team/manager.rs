//! Team Manager for sharing TeamExecutor across clients.
//!
//! This module provides a wrapper around TeamExecutor that allows it to be
//! shared across multiple client connections while maintaining team state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::api::unified::UnifiedClient;
use crate::engine::team::types::AgentTeam;
use crate::tools::trait_def::Tool;

/// Manages teams across multiple client connections.
///
/// This struct wraps the team storage functionality, allowing teams to be
/// created, listed, and managed from different clients.
pub struct TeamManager {
    /// Active teams being managed.
    teams: Arc<RwLock<HashMap<String, AgentTeam>>>,
    /// API client for creating executors.
    api_client: Arc<UnifiedClient>,
    /// Tools available to sub-agents.
    tools: Vec<Arc<dyn Tool>>,
    /// Default working directory.
    default_cwd: PathBuf,
    /// Default model.
    default_model: String,
}

impl TeamManager {
    /// Create a new TeamManager.
    pub fn new(
        api_client: Arc<UnifiedClient>,
        tools: Vec<Arc<dyn Tool>>,
        default_cwd: PathBuf,
        default_model: String,
    ) -> Self {
        Self {
            teams: Arc::new(RwLock::new(HashMap::new())),
            api_client,
            tools,
            default_cwd,
            default_model,
        }
    }

    /// Store a team in the manager.
    pub async fn store_team(&self, team: AgentTeam) {
        self.teams.write().await.insert(team.id.clone(), team);
    }

    /// Get a team by ID.
    pub async fn get_team(&self, team_id: &str) -> Option<AgentTeam> {
        self.teams.read().await.get(team_id).cloned()
    }

    /// Update a team.
    pub async fn update_team(&self, team: AgentTeam) {
        self.teams.write().await.insert(team.id.clone(), team);
    }

    /// List all teams.
    pub async fn list_teams(&self) -> Vec<AgentTeam> {
        self.teams.read().await.values().cloned().collect()
    }

    /// Remove a team.
    pub async fn remove_team(&self, team_id: &str) -> Option<AgentTeam> {
        self.teams.write().await.remove(team_id)
    }

    /// Abort a team.
    pub async fn abort_team(&self, team_id: &str) -> Option<AgentTeam> {
        let mut teams = self.teams.write().await;
        if let Some(team) = teams.get_mut(team_id) {
            team.abort("User requested abort".to_string());
            return Some(team.clone());
        }
        None
    }

    /// Get the API client.
    pub fn api_client(&self) -> Arc<UnifiedClient> {
        Arc::clone(&self.api_client)
    }

    /// Get the tools.
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.clone()
    }

    /// Get the default cwd.
    pub fn default_cwd(&self) -> PathBuf {
        self.default_cwd.clone()
    }

    /// Get the default model.
    pub fn default_model(&self) -> String {
        self.default_model.clone()
    }
}
