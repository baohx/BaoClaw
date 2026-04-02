use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;

use crate::models::message::Usage;
use crate::models::task::TaskState;

/// The core application state managed by the Rust process.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoreState {
    pub session_id: String,
    pub model: String,
    pub verbose: bool,
    pub tasks: HashMap<String, TaskState>,
    pub usage: Usage,
    pub total_cost_usd: f64,
}

/// A state change patch using JSON Pointer paths.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatePatch {
    pub path: String,
    pub op: PatchOp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum PatchOp {
    #[serde(rename = "replace")]
    Replace { value: Value },
    #[serde(rename = "add")]
    Add { value: Value },
    #[serde(rename = "remove")]
    Remove,
}

/// Manages the core state and broadcasts patches to subscribers.
pub struct StateManager {
    state: Arc<RwLock<CoreState>>,
    patch_tx: broadcast::Sender<Vec<StatePatch>>,
}

impl StateManager {
    /// Creates a new StateManager with the given initial state and a broadcast channel (capacity 256).
    pub fn new(initial: CoreState) -> Self {
        let (patch_tx, _) = broadcast::channel(256);
        Self {
            state: Arc::new(RwLock::new(initial)),
            patch_tx,
        }
    }

    /// Returns a clone of the current state.
    pub fn get(&self) -> CoreState {
        self.state.read().unwrap().clone()
    }

    /// Applies the updater function, generates patches by diffing old vs new state,
    /// broadcasts the patches, and returns them.
    pub fn update(&self, updater: impl FnOnce(&mut CoreState)) -> Vec<StatePatch> {
        let mut state = self.state.write().unwrap();
        let old = state.clone();
        updater(&mut state);
        let patches = diff_states(&old, &state);
        if !patches.is_empty() {
            // Ignore send errors (no active receivers is fine)
            let _ = self.patch_tx.send(patches.clone());
        }
        patches
    }

    /// Returns a new broadcast receiver for state patches.
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<StatePatch>> {
        self.patch_tx.subscribe()
    }

    /// Returns the full state as a JSON Value (for full sync).
    pub fn snapshot(&self) -> Value {
        let state = self.state.read().unwrap();
        serde_json::to_value(&*state).unwrap_or(Value::Null)
    }

    /// Convenience method to add or update a task.
    pub fn update_task(&self, task: TaskState) {
        self.update(|state| {
            state.tasks.insert(task.id.clone(), task);
        });
    }

    /// Convenience method to remove a task by ID.
    pub fn remove_task(&self, task_id: &str) {
        self.update(|state| {
            state.tasks.remove(task_id);
        });
    }

    /// Convenience method to update usage.
    pub fn update_usage(&self, usage: Usage) {
        self.update(|state| {
            state.usage = usage;
        });
    }
}

/// Compares two CoreState instances and generates patches for changed fields.
fn diff_states(old: &CoreState, new: &CoreState) -> Vec<StatePatch> {
    let mut patches = Vec::new();

    // Compare simple fields
    if old.session_id != new.session_id {
        patches.push(StatePatch {
            path: "/session_id".to_string(),
            op: PatchOp::Replace {
                value: Value::String(new.session_id.clone()),
            },
        });
    }

    if old.model != new.model {
        patches.push(StatePatch {
            path: "/model".to_string(),
            op: PatchOp::Replace {
                value: Value::String(new.model.clone()),
            },
        });
    }

    if old.verbose != new.verbose {
        patches.push(StatePatch {
            path: "/verbose".to_string(),
            op: PatchOp::Replace {
                value: Value::Bool(new.verbose),
            },
        });
    }

    if (old.total_cost_usd - new.total_cost_usd).abs() > f64::EPSILON {
        patches.push(StatePatch {
            path: "/total_cost_usd".to_string(),
            op: PatchOp::Replace {
                value: serde_json::json!(new.total_cost_usd),
            },
        });
    }

    // Compare usage fields
    diff_usage(&old.usage, &new.usage, &mut patches);

    // Compare tasks: added, removed, changed
    diff_tasks(&old.tasks, &new.tasks, &mut patches);

    patches
}

fn diff_usage(old: &Usage, new: &Usage, patches: &mut Vec<StatePatch>) {
    if old.input_tokens != new.input_tokens {
        patches.push(StatePatch {
            path: "/usage/input_tokens".to_string(),
            op: PatchOp::Replace {
                value: serde_json::json!(new.input_tokens),
            },
        });
    }
    if old.output_tokens != new.output_tokens {
        patches.push(StatePatch {
            path: "/usage/output_tokens".to_string(),
            op: PatchOp::Replace {
                value: serde_json::json!(new.output_tokens),
            },
        });
    }
    if old.cache_creation_input_tokens != new.cache_creation_input_tokens {
        patches.push(StatePatch {
            path: "/usage/cache_creation_input_tokens".to_string(),
            op: PatchOp::Replace {
                value: serde_json::to_value(&new.cache_creation_input_tokens)
                    .unwrap_or(Value::Null),
            },
        });
    }
    if old.cache_read_input_tokens != new.cache_read_input_tokens {
        patches.push(StatePatch {
            path: "/usage/cache_read_input_tokens".to_string(),
            op: PatchOp::Replace {
                value: serde_json::to_value(&new.cache_read_input_tokens)
                    .unwrap_or(Value::Null),
            },
        });
    }
}

fn diff_tasks(
    old: &HashMap<String, TaskState>,
    new: &HashMap<String, TaskState>,
    patches: &mut Vec<StatePatch>,
) {
    // Check for added or changed tasks
    for (id, new_task) in new {
        match old.get(id) {
            None => {
                // Task was added
                patches.push(StatePatch {
                    path: format!("/tasks/{}", id),
                    op: PatchOp::Add {
                        value: serde_json::to_value(new_task).unwrap_or(Value::Null),
                    },
                });
            }
            Some(old_task) => {
                // Task exists — check if it changed by comparing serialized form
                let old_val = serde_json::to_value(old_task).unwrap_or(Value::Null);
                let new_val = serde_json::to_value(new_task).unwrap_or(Value::Null);
                if old_val != new_val {
                    patches.push(StatePatch {
                        path: format!("/tasks/{}", id),
                        op: PatchOp::Replace {
                            value: new_val,
                        },
                    });
                }
            }
        }
    }

    // Check for removed tasks
    for id in old.keys() {
        if !new.contains_key(id) {
            patches.push(StatePatch {
                path: format!("/tasks/{}", id),
                op: PatchOp::Remove,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{TaskStatus, TaskType};
    use std::path::PathBuf;

    fn default_usage() -> Usage {
        Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }
    }

    fn default_state() -> CoreState {
        CoreState {
            session_id: "test-session".to_string(),
            model: "claude-3".to_string(),
            verbose: false,
            tasks: HashMap::new(),
            usage: default_usage(),
            total_cost_usd: 0.0,
        }
    }

    fn sample_task(id: &str) -> TaskState {
        TaskState {
            id: id.to_string(),
            task_type: TaskType::LocalBash,
            status: TaskStatus::Running,
            description: "test task".to_string(),
            tool_use_id: None,
            start_time: 1700000000000,
            end_time: None,
            output_file: PathBuf::from("/tmp/output.txt"),
            output_offset: 0,
        }
    }

    // --- State creation and get ---

    #[test]
    fn test_new_and_get() {
        let initial = default_state();
        let manager = StateManager::new(initial.clone());
        let state = manager.get();
        assert_eq!(state.session_id, "test-session");
        assert_eq!(state.model, "claude-3");
        assert!(!state.verbose);
        assert!(state.tasks.is_empty());
        assert_eq!(state.total_cost_usd, 0.0);
    }

    // --- Update with patch generation ---

    #[test]
    fn test_update_generates_patches_for_model_change() {
        let manager = StateManager::new(default_state());
        let patches = manager.update(|s| {
            s.model = "claude-4".to_string();
        });
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].path, "/model");
        match &patches[0].op {
            PatchOp::Replace { value } => assert_eq!(value, "claude-4"),
            _ => panic!("expected Replace"),
        }
    }

    #[test]
    fn test_update_no_patches_when_unchanged() {
        let manager = StateManager::new(default_state());
        let patches = manager.update(|_s| {
            // no changes
        });
        assert!(patches.is_empty());
    }

    #[test]
    fn test_update_multiple_field_changes() {
        let manager = StateManager::new(default_state());
        let patches = manager.update(|s| {
            s.model = "claude-4".to_string();
            s.verbose = true;
            s.total_cost_usd = 1.5;
        });
        assert_eq!(patches.len(), 3);
        let paths: Vec<&str> = patches.iter().map(|p| p.path.as_str()).collect();
        assert!(paths.contains(&"/model"));
        assert!(paths.contains(&"/verbose"));
        assert!(paths.contains(&"/total_cost_usd"));
    }

    // --- Task add/update/remove ---

    #[test]
    fn test_update_task_add() {
        let manager = StateManager::new(default_state());
        let task = sample_task("b12345678");
        manager.update_task(task);
        let state = manager.get();
        assert_eq!(state.tasks.len(), 1);
        assert!(state.tasks.contains_key("b12345678"));
    }

    #[test]
    fn test_update_task_modify() {
        let manager = StateManager::new(default_state());
        let task = sample_task("b12345678");
        manager.update_task(task);

        let mut updated = sample_task("b12345678");
        updated.status = TaskStatus::Completed;
        updated.end_time = Some(1700000001000);
        manager.update_task(updated);

        let state = manager.get();
        assert_eq!(state.tasks.len(), 1);
        assert_eq!(state.tasks["b12345678"].status, TaskStatus::Completed);
    }

    #[test]
    fn test_remove_task() {
        let manager = StateManager::new(default_state());
        manager.update_task(sample_task("b12345678"));
        assert_eq!(manager.get().tasks.len(), 1);

        manager.remove_task("b12345678");
        assert!(manager.get().tasks.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_task() {
        let manager = StateManager::new(default_state());
        // Should not panic
        manager.remove_task("nonexistent");
        assert!(manager.get().tasks.is_empty());
    }

    // --- Usage update ---

    #[test]
    fn test_update_usage() {
        let manager = StateManager::new(default_state());
        let new_usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: None,
        };
        manager.update_usage(new_usage);
        let state = manager.get();
        assert_eq!(state.usage.input_tokens, 100);
        assert_eq!(state.usage.output_tokens, 50);
        assert_eq!(state.usage.cache_creation_input_tokens, Some(10));
    }

    // --- Subscribe and receive patches ---

    #[tokio::test]
    async fn test_subscribe_receives_patches() {
        let manager = StateManager::new(default_state());
        let mut rx = manager.subscribe();

        manager.update(|s| {
            s.model = "claude-4".to_string();
        });

        let patches = rx.recv().await.unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].path, "/model");
    }

    #[tokio::test]
    async fn test_subscribe_receives_task_patches() {
        let manager = StateManager::new(default_state());
        let mut rx = manager.subscribe();

        manager.update_task(sample_task("b12345678"));

        let patches = rx.recv().await.unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].path, "/tasks/b12345678");
        assert!(matches!(&patches[0].op, PatchOp::Add { .. }));
    }

    #[tokio::test]
    async fn test_subscribe_no_broadcast_when_no_changes() {
        let manager = StateManager::new(default_state());
        let mut rx = manager.subscribe();

        manager.update(|_s| {
            // no changes
        });

        // No message should be sent; try_recv should fail
        assert!(rx.try_recv().is_err());
    }

    // --- Snapshot ---

    #[test]
    fn test_snapshot_returns_json_value() {
        let manager = StateManager::new(default_state());
        let snapshot = manager.snapshot();
        assert!(snapshot.is_object());
        assert_eq!(snapshot["session_id"], "test-session");
        assert_eq!(snapshot["model"], "claude-3");
        assert_eq!(snapshot["verbose"], false);
        assert_eq!(snapshot["total_cost_usd"], 0.0);
    }

    #[test]
    fn test_snapshot_reflects_updates() {
        let manager = StateManager::new(default_state());
        manager.update(|s| {
            s.model = "claude-4".to_string();
            s.total_cost_usd = 2.5;
        });
        let snapshot = manager.snapshot();
        assert_eq!(snapshot["model"], "claude-4");
        assert_eq!(snapshot["total_cost_usd"], 2.5);
    }

    // --- diff_states tests ---

    #[test]
    fn test_diff_session_id_change() {
        let old = default_state();
        let mut new = old.clone();
        new.session_id = "new-session".to_string();
        let patches = diff_states(&old, &new);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].path, "/session_id");
    }

    #[test]
    fn test_diff_usage_change() {
        let old = default_state();
        let mut new = old.clone();
        new.usage.input_tokens = 42;
        new.usage.output_tokens = 10;
        let patches = diff_states(&old, &new);
        assert_eq!(patches.len(), 2);
        let paths: Vec<&str> = patches.iter().map(|p| p.path.as_str()).collect();
        assert!(paths.contains(&"/usage/input_tokens"));
        assert!(paths.contains(&"/usage/output_tokens"));
    }

    #[test]
    fn test_diff_task_added() {
        let old = default_state();
        let mut new = old.clone();
        new.tasks.insert("b12345678".to_string(), sample_task("b12345678"));
        let patches = diff_states(&old, &new);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].path, "/tasks/b12345678");
        assert!(matches!(&patches[0].op, PatchOp::Add { .. }));
    }

    #[test]
    fn test_diff_task_removed() {
        let mut old = default_state();
        old.tasks.insert("b12345678".to_string(), sample_task("b12345678"));
        let new = default_state();
        let patches = diff_states(&old, &new);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].path, "/tasks/b12345678");
        assert!(matches!(&patches[0].op, PatchOp::Remove));
    }

    #[test]
    fn test_diff_task_changed() {
        let mut old = default_state();
        old.tasks.insert("b12345678".to_string(), sample_task("b12345678"));
        let mut new = old.clone();
        new.tasks.get_mut("b12345678").unwrap().status = TaskStatus::Completed;
        new.tasks.get_mut("b12345678").unwrap().end_time = Some(1700000001000);
        let patches = diff_states(&old, &new);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].path, "/tasks/b12345678");
        assert!(matches!(&patches[0].op, PatchOp::Replace { .. }));
    }

    #[test]
    fn test_diff_no_changes() {
        let state = default_state();
        let patches = diff_states(&state, &state);
        assert!(patches.is_empty());
    }

    // --- Serialization ---

    #[test]
    fn test_state_patch_serialization() {
        let patch = StatePatch {
            path: "/model".to_string(),
            op: PatchOp::Replace {
                value: Value::String("claude-4".to_string()),
            },
        };
        let json = serde_json::to_string(&patch).unwrap();
        let deserialized: StatePatch = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.path, "/model");
        assert!(matches!(deserialized.op, PatchOp::Replace { .. }));
    }

    #[test]
    fn test_core_state_serialization_roundtrip() {
        let mut state = default_state();
        state.tasks.insert("b12345678".to_string(), sample_task("b12345678"));
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: CoreState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, state.session_id);
        assert_eq!(deserialized.model, state.model);
        assert_eq!(deserialized.tasks.len(), 1);
    }
}
