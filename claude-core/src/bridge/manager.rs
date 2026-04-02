use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeConfig {
    pub dir: PathBuf,
    pub machine_name: String,
    pub branch: String,
    pub max_sessions: u32,
    pub spawn_mode: SpawnMode,
    pub api_base_url: String,
    pub session_timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SpawnMode {
    SingleSession,
    Worktree,
    SameDir,
}

#[derive(Clone, Debug)]
pub struct SessionHandle {
    pub session_id: String,
    pub status: SessionStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionStatus {
    Active,
    Completed,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct WorkAssignment {
    pub work_id: String,
    pub prompt: String,
    pub session_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Max sessions reached ({0})")]
    MaxSessionsReached(u32),
    #[error("Not registered")]
    NotRegistered,
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
}

pub struct BridgeManager {
    config: BridgeConfig,
    sessions: Arc<RwLock<HashMap<String, SessionHandle>>>,
    environment_id: Arc<RwLock<Option<String>>>,
}

impl BridgeManager {
    pub fn new(config: BridgeConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            environment_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Register this environment with the Bridge API.
    /// Returns (environment_id, environment_secret).
    /// Stub: actual HTTP calls will be wired up during integration.
    pub async fn register_environment(&self) -> Result<(String, String), BridgeError> {
        let env_id = format!("env-{}", uuid::Uuid::new_v4());
        let env_secret = format!("secret-{}", uuid::Uuid::new_v4());
        let mut id = self.environment_id.write().await;
        *id = Some(env_id.clone());
        Ok((env_id, env_secret))
    }

    /// Poll for new work assignments.
    /// Stub: returns None until real API integration.
    pub async fn poll_for_work(&self) -> Result<Option<WorkAssignment>, BridgeError> {
        let id = self.environment_id.read().await;
        if id.is_none() {
            return Err(BridgeError::NotRegistered);
        }
        // Stub: no work available yet
        Ok(None)
    }

    /// Acknowledge a work assignment.
    /// Stub: actual HTTP call will be wired up during integration.
    pub async fn acknowledge_work(&self, _work_id: &str) -> Result<(), BridgeError> {
        let id = self.environment_id.read().await;
        if id.is_none() {
            return Err(BridgeError::NotRegistered);
        }
        Ok(())
    }

    /// Spawn a new session. Returns error if max_sessions reached.
    pub async fn spawn_session(&self, prompt: String) -> Result<SessionHandle, BridgeError> {
        let active_count = self.active_session_count().await;
        if active_count >= self.config.max_sessions as usize {
            return Err(BridgeError::MaxSessionsReached(self.config.max_sessions));
        }

        let session_id = format!("session-{}", uuid::Uuid::new_v4());
        let handle = SessionHandle {
            session_id: session_id.clone(),
            status: SessionStatus::Active,
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), handle.clone());

        // Stub: actual session spawning (worktree creation, process launch, etc.)
        // will be implemented during integration. The prompt would be forwarded
        // to the new session's QueryEngine.
        let _ = prompt;

        Ok(handle)
    }

    /// Get the number of active sessions.
    pub async fn active_session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .filter(|s| s.status == SessionStatus::Active)
            .count()
    }

    /// Shutdown all sessions and clean up.
    pub async fn shutdown(&self) -> Result<(), BridgeError> {
        let mut sessions = self.sessions.write().await;
        for session in sessions.values_mut() {
            if session.status == SessionStatus::Active {
                session.status = SessionStatus::Completed;
            }
        }
        let mut id = self.environment_id.write().await;
        *id = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(max_sessions: u32) -> BridgeConfig {
        BridgeConfig {
            dir: PathBuf::from("/tmp/test"),
            machine_name: "test-machine".to_string(),
            branch: "main".to_string(),
            max_sessions,
            spawn_mode: SpawnMode::SingleSession,
            api_base_url: "https://api.example.com".to_string(),
            session_timeout_ms: 30000,
        }
    }

    #[tokio::test]
    async fn test_new_manager_has_no_sessions() {
        let manager = BridgeManager::new(test_config(3));
        assert_eq!(manager.active_session_count().await, 0);
    }

    #[tokio::test]
    async fn test_spawn_session_increments_count() {
        let manager = BridgeManager::new(test_config(3));
        let handle = manager.spawn_session("hello".to_string()).await.unwrap();
        assert_eq!(handle.status, SessionStatus::Active);
        assert_eq!(manager.active_session_count().await, 1);
    }

    #[tokio::test]
    async fn test_spawn_session_max_capacity() {
        let manager = BridgeManager::new(test_config(2));
        manager.spawn_session("s1".to_string()).await.unwrap();
        manager.spawn_session("s2".to_string()).await.unwrap();
        let result = manager.spawn_session("s3".to_string()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            BridgeError::MaxSessionsReached(n) => assert_eq!(n, 2),
            other => panic!("Expected MaxSessionsReached, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_spawn_session_zero_max() {
        let manager = BridgeManager::new(test_config(0));
        let result = manager.spawn_session("s1".to_string()).await;
        assert!(matches!(result, Err(BridgeError::MaxSessionsReached(0))));
    }

    #[tokio::test]
    async fn test_shutdown_marks_all_completed() {
        let manager = BridgeManager::new(test_config(5));
        manager.spawn_session("s1".to_string()).await.unwrap();
        manager.spawn_session("s2".to_string()).await.unwrap();
        assert_eq!(manager.active_session_count().await, 2);

        manager.shutdown().await.unwrap();
        assert_eq!(manager.active_session_count().await, 0);
    }

    #[tokio::test]
    async fn test_shutdown_allows_new_sessions_after() {
        let manager = BridgeManager::new(test_config(1));
        manager.spawn_session("s1".to_string()).await.unwrap();
        assert_eq!(manager.active_session_count().await, 1);

        manager.shutdown().await.unwrap();
        assert_eq!(manager.active_session_count().await, 0);

        // Can spawn again after shutdown
        manager.spawn_session("s2".to_string()).await.unwrap();
        assert_eq!(manager.active_session_count().await, 1);
    }

    #[tokio::test]
    async fn test_register_environment() {
        let manager = BridgeManager::new(test_config(3));
        let (env_id, env_secret) = manager.register_environment().await.unwrap();
        assert!(env_id.starts_with("env-"));
        assert!(env_secret.starts_with("secret-"));
    }

    #[tokio::test]
    async fn test_poll_for_work_requires_registration() {
        let manager = BridgeManager::new(test_config(3));
        let result = manager.poll_for_work().await;
        assert!(matches!(result, Err(BridgeError::NotRegistered)));
    }

    #[tokio::test]
    async fn test_poll_for_work_after_registration() {
        let manager = BridgeManager::new(test_config(3));
        manager.register_environment().await.unwrap();
        let result = manager.poll_for_work().await.unwrap();
        assert!(result.is_none()); // Stub returns None
    }

    #[tokio::test]
    async fn test_acknowledge_work_requires_registration() {
        let manager = BridgeManager::new(test_config(3));
        let result = manager.acknowledge_work("work-1").await;
        assert!(matches!(result, Err(BridgeError::NotRegistered)));
    }

    #[tokio::test]
    async fn test_acknowledge_work_after_registration() {
        let manager = BridgeManager::new(test_config(3));
        manager.register_environment().await.unwrap();
        manager.acknowledge_work("work-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_completed_sessions_dont_count_as_active() {
        let manager = BridgeManager::new(test_config(2));
        manager.spawn_session("s1".to_string()).await.unwrap();
        manager.spawn_session("s2".to_string()).await.unwrap();
        assert_eq!(manager.active_session_count().await, 2);

        // Manually mark one as completed
        {
            let mut sessions = manager.sessions.write().await;
            if let Some(session) = sessions.values_mut().next() {
                session.status = SessionStatus::Completed;
            }
        }

        assert_eq!(manager.active_session_count().await, 1);
        // Now we can spawn another since one slot freed up
        manager.spawn_session("s3".to_string()).await.unwrap();
        assert_eq!(manager.active_session_count().await, 2);
    }

    #[tokio::test]
    async fn test_failed_sessions_dont_count_as_active() {
        let manager = BridgeManager::new(test_config(1));
        let handle = manager.spawn_session("s1".to_string()).await.unwrap();

        {
            let mut sessions = manager.sessions.write().await;
            sessions.get_mut(&handle.session_id).unwrap().status =
                SessionStatus::Failed("error".to_string());
        }

        assert_eq!(manager.active_session_count().await, 0);
        // Can spawn a new one
        manager.spawn_session("s2".to_string()).await.unwrap();
        assert_eq!(manager.active_session_count().await, 1);
    }
}
