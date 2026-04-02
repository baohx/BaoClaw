use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::message::ValidationError;

// --- Task types ---

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub description: String,
    pub tool_use_id: Option<String>,
    pub start_time: u64,
    pub end_time: Option<u64>,
    pub output_file: PathBuf,
    pub output_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TaskType {
    LocalBash,
    LocalAgent,
    RemoteAgent,
    InProcessTeammate,
    LocalWorkflow,
    MonitorMcp,
    Dream,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

impl TaskStatus {
    /// Returns true if the status is a terminal state (Completed, Failed, Killed).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }

    /// Validates whether a transition from the current status to `next` is allowed.
    /// Valid transitions: Pending → Running, Running → Completed/Failed/Killed.
    pub fn can_transition_to(&self, next: &TaskStatus) -> bool {
        matches!(
            (self, next),
            (TaskStatus::Pending, TaskStatus::Running)
                | (TaskStatus::Running, TaskStatus::Completed)
                | (TaskStatus::Running, TaskStatus::Failed)
                | (TaskStatus::Running, TaskStatus::Killed)
        )
    }
}

impl TaskType {
    /// Returns the single-character prefix for task ID generation.
    pub fn id_prefix(&self) -> char {
        match self {
            TaskType::LocalBash => 'b',
            TaskType::LocalAgent => 'a',
            TaskType::RemoteAgent => 'r',
            TaskType::InProcessTeammate => 't',
            TaskType::LocalWorkflow => 'w',
            TaskType::MonitorMcp => 'm',
            TaskType::Dream => 'd',
        }
    }
}

/// Generates a task ID with the correct type prefix + 8 random alphanumeric chars [0-9a-z].
pub fn generate_task_id(task_type: &TaskType) -> String {
    let prefix = task_type.id_prefix();
    let mut rng = rand::thread_rng();
    let suffix: String = (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..36u8);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect();
    format!("{}{}", prefix, suffix)
}

/// Validates a task ID: must be 9 chars, first char is a valid prefix, remaining 8 are [0-9a-z].
pub fn is_valid_task_id(id: &str) -> bool {
    if id.len() != 9 {
        return false;
    }
    let prefix = id.chars().next().unwrap();
    if !matches!(prefix, 'b' | 'a' | 'r' | 't' | 'w' | 'm' | 'd') {
        return false;
    }
    id[1..].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Returns the expected prefix character for a given TaskType.
fn expected_prefix(task_type: &TaskType) -> char {
    task_type.id_prefix()
}

impl TaskState {
    /// Validates the task state for consistency.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Validate ID format
        if !is_valid_task_id(&self.id) {
            return Err(ValidationError {
                field: "id".to_string(),
                message: "task ID must be 9 chars: valid prefix + 8 alphanumeric [0-9a-z]"
                    .to_string(),
            });
        }

        // Validate ID prefix matches task type
        let prefix = self.id.chars().next().unwrap();
        if prefix != expected_prefix(&self.task_type) {
            return Err(ValidationError {
                field: "id".to_string(),
                message: format!(
                    "task ID prefix '{}' does not match task type {:?} (expected '{}')",
                    prefix,
                    self.task_type,
                    expected_prefix(&self.task_type)
                ),
            });
        }

        // Validate end_time consistency with terminal state
        if self.status.is_terminal() && self.end_time.is_none() {
            return Err(ValidationError {
                field: "end_time".to_string(),
                message: "end_time must be set when status is terminal".to_string(),
            });
        }
        if !self.status.is_terminal() && self.end_time.is_some() {
            return Err(ValidationError {
                field: "end_time".to_string(),
                message: "end_time must be None when status is not terminal".to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_terminal tests ---

    #[test]
    fn test_is_terminal_completed() {
        assert!(TaskStatus::Completed.is_terminal());
    }

    #[test]
    fn test_is_terminal_failed() {
        assert!(TaskStatus::Failed.is_terminal());
    }

    #[test]
    fn test_is_terminal_killed() {
        assert!(TaskStatus::Killed.is_terminal());
    }

    #[test]
    fn test_is_not_terminal_pending() {
        assert!(!TaskStatus::Pending.is_terminal());
    }

    #[test]
    fn test_is_not_terminal_running() {
        assert!(!TaskStatus::Running.is_terminal());
    }

    // --- can_transition_to tests ---

    #[test]
    fn test_pending_to_running() {
        assert!(TaskStatus::Pending.can_transition_to(&TaskStatus::Running));
    }

    #[test]
    fn test_running_to_completed() {
        assert!(TaskStatus::Running.can_transition_to(&TaskStatus::Completed));
    }

    #[test]
    fn test_running_to_failed() {
        assert!(TaskStatus::Running.can_transition_to(&TaskStatus::Failed));
    }

    #[test]
    fn test_running_to_killed() {
        assert!(TaskStatus::Running.can_transition_to(&TaskStatus::Killed));
    }

    #[test]
    fn test_invalid_pending_to_completed() {
        assert!(!TaskStatus::Pending.can_transition_to(&TaskStatus::Completed));
    }

    #[test]
    fn test_invalid_completed_to_running() {
        assert!(!TaskStatus::Completed.can_transition_to(&TaskStatus::Running));
    }

    #[test]
    fn test_invalid_running_to_pending() {
        assert!(!TaskStatus::Running.can_transition_to(&TaskStatus::Pending));
    }

    #[test]
    fn test_invalid_same_state_pending() {
        assert!(!TaskStatus::Pending.can_transition_to(&TaskStatus::Pending));
    }

    #[test]
    fn test_invalid_same_state_running() {
        assert!(!TaskStatus::Running.can_transition_to(&TaskStatus::Running));
    }

    // --- generate_task_id tests ---

    #[test]
    fn test_generate_task_id_format() {
        let all_types = vec![
            (TaskType::LocalBash, 'b'),
            (TaskType::LocalAgent, 'a'),
            (TaskType::RemoteAgent, 'r'),
            (TaskType::InProcessTeammate, 't'),
            (TaskType::LocalWorkflow, 'w'),
            (TaskType::MonitorMcp, 'm'),
            (TaskType::Dream, 'd'),
        ];
        for (task_type, expected_prefix) in all_types {
            let id = generate_task_id(&task_type);
            assert_eq!(id.len(), 9, "ID length should be 9 for {:?}", task_type);
            assert_eq!(
                id.chars().next().unwrap(),
                expected_prefix,
                "prefix mismatch for {:?}",
                task_type
            );
            assert!(
                id[1..].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "suffix should be [0-9a-z] for {:?}",
                task_type
            );
        }
    }

    #[test]
    fn test_generate_task_id_uniqueness() {
        let id1 = generate_task_id(&TaskType::LocalBash);
        let id2 = generate_task_id(&TaskType::LocalBash);
        // Extremely unlikely to collide with 36^8 possibilities
        assert_ne!(id1, id2);
    }

    // --- TaskState validation tests ---

    #[test]
    fn test_validate_valid_pending_task() {
        let task = TaskState {
            id: generate_task_id(&TaskType::LocalBash),
            task_type: TaskType::LocalBash,
            status: TaskStatus::Pending,
            description: "run ls".to_string(),
            tool_use_id: None,
            start_time: 1700000000000,
            end_time: None,
            output_file: PathBuf::from("/tmp/output.txt"),
            output_offset: 0,
        };
        assert!(task.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_completed_task() {
        let task = TaskState {
            id: generate_task_id(&TaskType::LocalAgent),
            task_type: TaskType::LocalAgent,
            status: TaskStatus::Completed,
            description: "agent task".to_string(),
            tool_use_id: Some("tu_123".to_string()),
            start_time: 1700000000000,
            end_time: Some(1700000001000),
            output_file: PathBuf::from("/tmp/output.txt"),
            output_offset: 0,
        };
        assert!(task.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_id_format() {
        let task = TaskState {
            id: "invalid".to_string(),
            task_type: TaskType::LocalBash,
            status: TaskStatus::Pending,
            description: "test".to_string(),
            tool_use_id: None,
            start_time: 1700000000000,
            end_time: None,
            output_file: PathBuf::from("/tmp/output.txt"),
            output_offset: 0,
        };
        let err = task.validate().unwrap_err();
        assert_eq!(err.field, "id");
    }

    #[test]
    fn test_validate_id_prefix_mismatch() {
        // Create an ID with 'b' prefix but assign LocalAgent type
        let task = TaskState {
            id: "b12345678".to_string(),
            task_type: TaskType::LocalAgent, // expects 'a' prefix
            status: TaskStatus::Pending,
            description: "test".to_string(),
            tool_use_id: None,
            start_time: 1700000000000,
            end_time: None,
            output_file: PathBuf::from("/tmp/output.txt"),
            output_offset: 0,
        };
        let err = task.validate().unwrap_err();
        assert_eq!(err.field, "id");
    }

    #[test]
    fn test_validate_terminal_without_end_time() {
        let task = TaskState {
            id: generate_task_id(&TaskType::LocalBash),
            task_type: TaskType::LocalBash,
            status: TaskStatus::Completed,
            description: "test".to_string(),
            tool_use_id: None,
            start_time: 1700000000000,
            end_time: None, // should be set for terminal state
            output_file: PathBuf::from("/tmp/output.txt"),
            output_offset: 0,
        };
        let err = task.validate().unwrap_err();
        assert_eq!(err.field, "end_time");
    }

    #[test]
    fn test_validate_non_terminal_with_end_time() {
        let task = TaskState {
            id: generate_task_id(&TaskType::LocalBash),
            task_type: TaskType::LocalBash,
            status: TaskStatus::Running,
            description: "test".to_string(),
            tool_use_id: None,
            start_time: 1700000000000,
            end_time: Some(1700000001000), // should be None for non-terminal
            output_file: PathBuf::from("/tmp/output.txt"),
            output_offset: 0,
        };
        let err = task.validate().unwrap_err();
        assert_eq!(err.field, "end_time");
    }

    // --- Serialization round-trip test ---

    #[test]
    fn test_task_state_serialization_roundtrip() {
        let task = TaskState {
            id: "b12345678".to_string(),
            task_type: TaskType::LocalBash,
            status: TaskStatus::Running,
            description: "run ls -la".to_string(),
            tool_use_id: Some("tu_abc".to_string()),
            start_time: 1700000000000,
            end_time: None,
            output_file: PathBuf::from("/tmp/task_output.txt"),
            output_offset: 42,
        };
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: TaskState = serde_json::from_str(&json).unwrap();
        assert_eq!(task.id, deserialized.id);
        assert_eq!(task.task_type, deserialized.task_type);
        assert_eq!(task.status, deserialized.status);
        assert_eq!(task.description, deserialized.description);
        assert_eq!(task.tool_use_id, deserialized.tool_use_id);
        assert_eq!(task.start_time, deserialized.start_time);
        assert_eq!(task.end_time, deserialized.end_time);
        assert_eq!(task.output_file, deserialized.output_file);
        assert_eq!(task.output_offset, deserialized.output_offset);
    }
}
