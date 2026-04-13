use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};
use serde::{Serialize, Deserialize};

use crate::api::unified::UnifiedClient;
use crate::engine::query_engine::{EngineEvent, QueryEngine, QueryEngineConfig, ThinkingConfig};
use crate::tools::trait_def::Tool;

/// Status of a background task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BgTaskStatus {
    Running,
    Completed,
    Failed(String),
    Aborted,
}

/// A background task that runs an independent QueryEngine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: String,
    pub description: String,
    pub status: BgTaskStatus,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub result: Option<String>,
}

/// Manages background tasks with independent QueryEngine instances.
pub struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, BackgroundTask>>>,
    abort_handles: Arc<RwLock<HashMap<String, watch::Sender<bool>>>>,
    api_client: Arc<UnifiedClient>,
    tools: Vec<Arc<dyn Tool>>,
}

impl TaskManager {
    /// Create a new TaskManager.
    pub fn new(api_client: Arc<UnifiedClient>, tools: Vec<Arc<dyn Tool>>) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            abort_handles: Arc::new(RwLock::new(HashMap::new())),
            api_client,
            tools,
        }
    }

    /// Create and spawn a background task. Returns the task ID.
    pub async fn create_task(
        &self,
        description: String,
        prompt: String,
        cwd: std::path::PathBuf,
        model: String,
    ) -> String {
        let task_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let task = BackgroundTask {
            id: task_id.clone(),
            description: description.clone(),
            status: BgTaskStatus::Running,
            created_at: now,
            completed_at: None,
            result: None,
        };

        // Store the task
        self.tasks.write().await.insert(task_id.clone(), task);

        // Create abort channel for this task
        let (abort_tx, _abort_rx) = watch::channel(false);
        self.abort_handles.write().await.insert(task_id.clone(), abort_tx);

        // Clone what we need for the spawned task
        let tasks = Arc::clone(&self.tasks);
        let abort_handles = Arc::clone(&self.abort_handles);
        let api_client = Arc::clone(&self.api_client);
        let tools = self.tools.clone();
        let tid = task_id.clone();

        tokio::spawn(async move {
            let config = QueryEngineConfig {
                cwd,
                tools,
                api_client,
                model,
                thinking_config: ThinkingConfig::Disabled,
                max_turns: Some(20),
                max_budget_usd: None,
                verbose: false,
                custom_system_prompt: Some(
                    "You are a background task agent. Complete the given task efficiently.".to_string()
                ),
                append_system_prompt: None,
                session_id: None,
                fallback_models: vec![],
                max_retries_per_model: 2,
            };

            let mut engine = QueryEngine::new(config);
            let mut rx = engine.submit_message(prompt).await;

            let mut final_text = String::new();
            let mut error_msg: Option<String> = None;

            while let Some(event) = rx.recv().await {
                match event {
                    EngineEvent::AssistantChunk { content, .. } => {
                        final_text.push_str(&content);
                    }
                    EngineEvent::Result(result) => {
                        if let Some(text) = result.text {
                            final_text = text;
                        }
                        break;
                    }
                    EngineEvent::Error(err) => {
                        error_msg = Some(err.message);
                        break;
                    }
                    _ => {}
                }
            }

            // Update task status
            let now = chrono::Utc::now().to_rfc3339();
            let mut tasks_guard = tasks.write().await;
            if let Some(task) = tasks_guard.get_mut(&tid) {
                if task.status == BgTaskStatus::Aborted {
                    // Already aborted, don't overwrite
                } else if let Some(err) = error_msg {
                    task.status = BgTaskStatus::Failed(err);
                    task.completed_at = Some(now);
                } else {
                    task.status = BgTaskStatus::Completed;
                    task.result = Some(final_text);
                    task.completed_at = Some(now);
                }
            }

            // Clean up abort handle
            abort_handles.write().await.remove(&tid);
        });

        task_id
    }

    /// List all tasks.
    pub async fn list_tasks(&self) -> Vec<BackgroundTask> {
        self.tasks.read().await.values().cloned().collect()
    }

    /// Get the status of a specific task.
    pub async fn get_task_status(&self, task_id: &str) -> Option<BackgroundTask> {
        self.tasks.read().await.get(task_id).cloned()
    }

    /// Stop a running task by sending an abort signal.
    pub async fn stop_task(&self, task_id: &str) -> bool {
        // Send abort signal
        let sent = if let Some(tx) = self.abort_handles.read().await.get(task_id) {
            tx.send(true).is_ok()
        } else {
            false
        };

        // Update task status to Aborted
        if let Some(task) = self.tasks.write().await.get_mut(task_id) {
            if task.status == BgTaskStatus::Running {
                task.status = BgTaskStatus::Aborted;
                task.completed_at = Some(chrono::Utc::now().to_rfc3339());
                return true;
            }
        }

        sent
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
        }))
    }

    #[tokio::test]
    async fn test_create_task_returns_id() {
        let api_client = make_api_client();
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let manager = TaskManager::new(api_client, tools);

        let task_id = manager.create_task(
            "test task".to_string(),
            "do something".to_string(),
            std::path::PathBuf::from("/tmp"),
            "claude-sonnet-4-20250514".to_string(),
        ).await;

        assert!(!task_id.is_empty());
        assert_eq!(task_id.len(), 8); // UUID first 8 chars
    }

    #[tokio::test]
    async fn test_list_tasks_after_create() {
        let api_client = make_api_client();
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let manager = TaskManager::new(api_client, tools);

        let task_id = manager.create_task(
            "test task".to_string(),
            "do something".to_string(),
            std::path::PathBuf::from("/tmp"),
            "claude-sonnet-4-20250514".to_string(),
        ).await;

        let tasks = manager.list_tasks().await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task_id);
        assert_eq!(tasks[0].description, "test task");
    }

    #[tokio::test]
    async fn test_get_task_status() {
        let api_client = make_api_client();
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let manager = TaskManager::new(api_client, tools);

        let task_id = manager.create_task(
            "test task".to_string(),
            "do something".to_string(),
            std::path::PathBuf::from("/tmp"),
            "claude-sonnet-4-20250514".to_string(),
        ).await;

        let task = manager.get_task_status(&task_id).await;
        assert!(task.is_some());
        let task = task.unwrap();
        assert_eq!(task.id, task_id);
        assert_eq!(task.description, "test task");
    }

    #[tokio::test]
    async fn test_get_task_status_not_found() {
        let api_client = make_api_client();
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let manager = TaskManager::new(api_client, tools);

        let task = manager.get_task_status("nonexistent").await;
        assert!(task.is_none());
    }

    #[tokio::test]
    async fn test_stop_task() {
        let api_client = make_api_client();
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let manager = TaskManager::new(api_client, tools);

        let task_id = manager.create_task(
            "test task".to_string(),
            "do something".to_string(),
            std::path::PathBuf::from("/tmp"),
            "claude-sonnet-4-20250514".to_string(),
        ).await;

        let stopped = manager.stop_task(&task_id).await;
        assert!(stopped);

        let task = manager.get_task_status(&task_id).await.unwrap();
        assert_eq!(task.status, BgTaskStatus::Aborted);
        assert!(task.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_stop_nonexistent_task() {
        let api_client = make_api_client();
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let manager = TaskManager::new(api_client, tools);

        let stopped = manager.stop_task("nonexistent").await;
        assert!(!stopped);
    }

    #[tokio::test]
    async fn test_list_empty_tasks() {
        let api_client = make_api_client();
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let manager = TaskManager::new(api_client, tools);

        let tasks = manager.list_tasks().await;
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn test_background_task_serialization() {
        let task = BackgroundTask {
            id: "abc12345".to_string(),
            description: "test".to_string(),
            status: BgTaskStatus::Running,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            completed_at: None,
            result: None,
        };
        let json = serde_json::to_value(&task).unwrap();
        assert_eq!(json["id"], "abc12345");
        assert_eq!(json["status"], "Running");

        let failed = BackgroundTask {
            status: BgTaskStatus::Failed("oops".to_string()),
            ..task
        };
        let json = serde_json::to_value(&failed).unwrap();
        assert_eq!(json["status"]["Failed"], "oops");
    }
}
