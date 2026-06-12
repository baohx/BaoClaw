//! Memory decay algorithm implementation.
//!
//! This module implements the automatic memory importance decay mechanism.
//! Memories that are frequently accessed remain relevant, while unused memories
//! gradually fade and eventually get archived.

use serde::{Deserialize, Serialize};

use crate::engine::memory::MemoryEntry;

/// Default decay rate per day (0.99 = 1% decay per day).
const DEFAULT_DECAY_RATE: f64 = 0.99;

/// Default boost when memory is recalled (0.1).
const DEFAULT_RECALL_BOOST: f64 = 0.1;

/// Default boost when user confirms memory relevance (0.2).
const DEFAULT_CONFIRM_BOOST: f64 = 0.2;

/// Default penalty when user rejects memory relevance (0.3).
const DEFAULT_REJECT_PENALTY: f64 = 0.3;

/// Default threshold below which memories are archived (0.1).
const DEFAULT_ARCHIVE_THRESHOLD: f64 = 0.1;

/// Default maximum number of memory entries before cleanup is triggered.
const DEFAULT_MAX_ENTRIES: usize = 1000;

/// Default cleanup interval in hours (24 = daily).
const DEFAULT_CLEANUP_INTERVAL_HOURS: u64 = 24;

fn default_cleanup_interval_hours() -> u64 {
    DEFAULT_CLEANUP_INTERVAL_HOURS
}

/// Configuration for memory decay behavior.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DecayConfig {
    /// Decay rate per day (e.g., 0.99 means 1% decay per day).
    /// Higher values = slower decay.
    #[serde(default = "default_decay_rate")]
    pub decay_rate: f64,

    /// Importance boost when memory is recalled.
    #[serde(default = "default_recall_boost")]
    pub recall_boost: f64,

    /// Importance boost when user confirms memory relevance.
    #[serde(default = "default_confirm_boost")]
    pub confirm_boost: f64,

    /// Importance penalty when user rejects memory relevance.
    #[serde(default = "default_reject_penalty")]
    pub reject_penalty: f64,

    /// Importance threshold below which memories are archived.
    #[serde(default = "default_archive_threshold")]
    pub archive_threshold: f64,

    /// Maximum number of memory entries before cleanup.
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// Interval in hours between cleanup runs.
    /// Default: 24 (daily).
    #[serde(default = "default_cleanup_interval_hours")]
    pub cleanup_interval_hours: u64,
}

fn default_decay_rate() -> f64 {
    DEFAULT_DECAY_RATE
}

fn default_recall_boost() -> f64 {
    DEFAULT_RECALL_BOOST
}

fn default_confirm_boost() -> f64 {
    DEFAULT_CONFIRM_BOOST
}

fn default_reject_penalty() -> f64 {
    DEFAULT_REJECT_PENALTY
}

fn default_archive_threshold() -> f64 {
    DEFAULT_ARCHIVE_THRESHOLD
}

fn default_max_entries() -> usize {
    DEFAULT_MAX_ENTRIES
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            decay_rate: DEFAULT_DECAY_RATE,
            recall_boost: DEFAULT_RECALL_BOOST,
            confirm_boost: DEFAULT_CONFIRM_BOOST,
            reject_penalty: DEFAULT_REJECT_PENALTY,
            archive_threshold: DEFAULT_ARCHIVE_THRESHOLD,
            max_entries: DEFAULT_MAX_ENTRIES,
            cleanup_interval_hours: DEFAULT_CLEANUP_INTERVAL_HOURS,
        }
    }
}

impl DecayConfig {
    /// Create a new DecayConfig with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load decay config from the main config file's "memory" section.
    /// Returns default config if not found or on error.
    pub fn load() -> Self {
        let config_path = crate::config::config_path();
        match std::fs::read_to_string(&config_path) {
            Ok(content) => {
                // Try to parse the memory section from the config
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(memory) = json.get("memory") {
                        if let Ok(config) = serde_json::from_value(memory.clone()) {
                            return config;
                        }
                    }
                }
                Self::default()
            }
            Err(_) => Self::default(),
        }
    }
}

/// Calculate the decayed importance score after a given number of days.
///
/// The decay formula is: `score = importance * decay_rate^days`
///
/// # Arguments
/// * `memory` - The memory entry to calculate decay for
/// * `days` - Number of days since the memory was last updated/recalled
/// * `config` - Decay configuration parameters
///
/// # Returns
/// The decayed importance score (0.0 to 1.0)
///
/// # Example
/// ```ignore
/// let memory = MemoryEntry { importance: 1.0, ... };
/// let config = DecayConfig::default();
/// let decayed = decay_score(&memory, 10.0, &config);
/// // After 10 days: 1.0 * 0.99^10 ≈ 0.904
/// ```
pub fn decay_score(memory: &MemoryEntry, days: f64, config: &DecayConfig) -> f64 {
    let decayed = memory.importance * config.decay_rate.powf(days);
    // Clamp to valid range [0.0, 1.0]
    decayed.clamp(0.0, 1.0)
}

/// Boost importance when memory is recalled.
///
/// Increases the memory's importance by `recall_boost`, increments the recall count,
/// and updates the last_recalled_at timestamp.
///
/// # Arguments
/// * `memory` - The memory entry to boost (modified in place)
/// * `config` - Decay configuration parameters
///
/// # Example
/// ```ignore
/// let mut memory = MemoryEntry { importance: 0.5, recall_count: 0, ... };
/// let config = DecayConfig::default();
/// boost_on_recall(&mut memory, &config);
/// // importance is now min(0.5 + 0.1, 1.0) = 0.6
/// // recall_count is now 1
/// ```
pub fn boost_on_recall(memory: &mut MemoryEntry, config: &DecayConfig) {
    memory.importance = (memory.importance + config.recall_boost).min(1.0);
    memory.recall_count += 1;
    memory.last_recalled_at = Some(now_iso8601());
}

/// Boost importance when user confirms memory relevance.
///
/// Called when a user explicitly confirms that a memory is relevant/accurate.
/// Provides a larger boost than recall.
///
/// # Arguments
/// * `memory` - The memory entry to boost (modified in place)
/// * `config` - Decay configuration parameters
///
/// # Example
/// ```ignore
/// let mut memory = MemoryEntry { importance: 0.5, ... };
/// let config = DecayConfig::default();
/// boost_on_confirm(&mut memory, &config);
/// // importance is now min(0.5 + 0.2, 1.0) = 0.7
/// ```
pub fn boost_on_confirm(memory: &mut MemoryEntry, config: &DecayConfig) {
    memory.importance = (memory.importance + config.confirm_boost).min(1.0);
    // Also update recall info
    memory.recall_count += 1;
    memory.last_recalled_at = Some(now_iso8601());
}

/// Reduce importance when user rejects memory relevance.
///
/// Called when a user indicates that a memory is no longer relevant or accurate.
/// Reduces the importance, potentially leading to archival.
///
/// # Arguments
/// * `memory` - The memory entry to penalize (modified in place)
/// * `config` - Decay configuration parameters
///
/// # Example
/// ```ignore
/// let mut memory = MemoryEntry { importance: 0.5, ... };
/// let config = DecayConfig::default();
/// penalize_on_reject(&mut memory, &config);
/// // importance is now max(0.5 - 0.3, 0.0) = 0.2
/// ```
pub fn penalize_on_reject(memory: &mut MemoryEntry, config: &DecayConfig) {
    memory.importance = (memory.importance - config.reject_penalty).max(0.0);
}

/// Check if a memory should be archived based on its importance.
///
/// # Arguments
/// * `memory` - The memory entry to check
/// * `config` - Decay configuration parameters
///
/// # Returns
/// `true` if the memory's importance is below the archive threshold
pub fn should_archive(memory: &MemoryEntry, config: &DecayConfig) -> bool {
    memory.importance < config.archive_threshold
}

/// Apply decay to all memories and return those that should be archived.
///
/// This is the main entry point for the periodic cleanup task.
/// For each memory:
/// 1. Calculate days since last recall (or creation if never recalled)
/// 2. Apply decay formula
/// 3. Check if it should be archived
///
/// # Arguments
/// * `memories` - List of memory entries to process (modified in place)
/// * `config` - Decay configuration parameters
///
/// # Returns
/// A vector of memory IDs that should be archived
pub fn apply_decay(memories: &mut [MemoryEntry], config: &DecayConfig) -> Vec<String> {
    let now = chrono::Utc::now();
    let mut to_archive = Vec::new();

    for memory in memories.iter_mut() {
        // Skip already archived memories
        if memory.archived {
            continue;
        }

        // Calculate days since last update
        let last_time = memory.last_recalled_at.as_ref()
            .or(Some(&memory.created_at))
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let days = match last_time {
            Some(last) => (now - last).num_days() as f64,
            None => 0.0, // If we can't parse, assume no decay needed
        };

        // Apply decay
        memory.importance = decay_score(memory, days, config);

        // Check if should be archived
        if should_archive(memory, config) {
            memory.archived = true;
            to_archive.push(memory.id.clone());
        }
    }

    to_archive
}

/// Get current time as ISO8601 string.
fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::memory::MemoryCategory;

    fn create_test_memory(importance: f64) -> MemoryEntry {
        MemoryEntry {
            id: "test-id".to_string(),
            content: "test content".to_string(),
            category: MemoryCategory::Fact,
            created_at: chrono::Utc::now().to_rfc3339(),
            source: "test".to_string(),
            importance,
            recall_count: 0,
            last_recalled_at: None,
            archived: false,
        }
    }

    #[test]
    fn test_default_config_values() {
        let config = DecayConfig::default();
        assert!((config.decay_rate - 0.99).abs() < f64::EPSILON);
        assert!((config.recall_boost - 0.1).abs() < f64::EPSILON);
        assert!((config.confirm_boost - 0.2).abs() < f64::EPSILON);
        assert!((config.reject_penalty - 0.3).abs() < f64::EPSILON);
        assert!((config.archive_threshold - 0.1).abs() < f64::EPSILON);
        assert_eq!(config.max_entries, 1000);
        assert_eq!(config.cleanup_interval_hours, 24);
    }

    #[test]
    fn test_decay_score_no_decay() {
        let memory = create_test_memory(0.5);
        let config = DecayConfig::default();
        let result = decay_score(&memory, 0.0, &config);
        assert!((result - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_decay_score_one_day() {
        let memory = create_test_memory(1.0);
        let config = DecayConfig::default();
        let result = decay_score(&memory, 1.0, &config);
        // 1.0 * 0.99^1 = 0.99
        assert!((result - 0.99).abs() < 0.001);
    }

    #[test]
    fn test_decay_score_ten_days() {
        let memory = create_test_memory(1.0);
        let config = DecayConfig::default();
        let result = decay_score(&memory, 10.0, &config);
        // 1.0 * 0.99^10 ≈ 0.904
        assert!((result - 0.904).abs() < 0.01);
    }

    #[test]
    fn test_decay_score_thirty_days() {
        let memory = create_test_memory(1.0);
        let config = DecayConfig::default();
        let result = decay_score(&memory, 30.0, &config);
        // 1.0 * 0.99^30 ≈ 0.739
        assert!((result - 0.739).abs() < 0.01);
    }

    #[test]
    fn test_boost_on_recall() {
        let mut memory = create_test_memory(0.5);
        let config = DecayConfig::default();
        boost_on_recall(&mut memory, &config);
        assert!((memory.importance - 0.6).abs() < f64::EPSILON);
        assert_eq!(memory.recall_count, 1);
        assert!(memory.last_recalled_at.is_some());
    }

    #[test]
    fn test_boost_on_recall_caps_at_one() {
        let mut memory = create_test_memory(0.95);
        let config = DecayConfig::default();
        boost_on_recall(&mut memory, &config);
        assert!((memory.importance - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_boost_on_confirm() {
        let mut memory = create_test_memory(0.5);
        let config = DecayConfig::default();
        boost_on_confirm(&mut memory, &config);
        assert!((memory.importance - 0.7).abs() < f64::EPSILON);
        assert_eq!(memory.recall_count, 1);
        assert!(memory.last_recalled_at.is_some());
    }

    #[test]
    fn test_boost_on_confirm_caps_at_one() {
        let mut memory = create_test_memory(0.9);
        let config = DecayConfig::default();
        boost_on_confirm(&mut memory, &config);
        assert!((memory.importance - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_penalize_on_reject() {
        let mut memory = create_test_memory(0.5);
        let config = DecayConfig::default();
        penalize_on_reject(&mut memory, &config);
        assert!((memory.importance - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_penalize_on_reject_floors_at_zero() {
        let mut memory = create_test_memory(0.1);
        let config = DecayConfig::default();
        penalize_on_reject(&mut memory, &config);
        assert!((memory.importance - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_should_archive_below_threshold() {
        let memory = create_test_memory(0.05);
        let config = DecayConfig::default();
        assert!(should_archive(&memory, &config));
    }

    #[test]
    fn test_should_archive_above_threshold() {
        let memory = create_test_memory(0.5);
        let config = DecayConfig::default();
        assert!(!should_archive(&memory, &config));
    }

    #[test]
    fn test_should_archive_at_threshold() {
        let memory = create_test_memory(0.1);
        let config = DecayConfig::default();
        // At threshold is NOT below threshold, so should not archive
        assert!(!should_archive(&memory, &config));
    }

    #[test]
    fn test_apply_decay_marks_for_archive() {
        let mut memories = vec![
            MemoryEntry {
                id: "old".to_string(),
                content: "old memory".to_string(),
                category: MemoryCategory::Fact,
                created_at: "2020-01-01T00:00:00Z".to_string(), // Very old
                source: "test".to_string(),
                importance: 0.5,
                recall_count: 0,
                last_recalled_at: None,
                archived: false,
            },
        ];
        let config = DecayConfig::default();
        let to_archive = apply_decay(&mut memories, &config);
        
        // After many years of decay, this should be archived
        assert!(memories[0].archived);
        assert_eq!(to_archive, vec!["old".to_string()]);
    }

    #[test]
    fn test_apply_decay_skips_archived() {
        let mut memories = vec![
            MemoryEntry {
                id: "already-archived".to_string(),
                content: "archived memory".to_string(),
                category: MemoryCategory::Fact,
                created_at: "2020-01-01T00:00:00Z".to_string(),
                source: "test".to_string(),
                importance: 0.01, // Very low
                recall_count: 0,
                last_recalled_at: None,
                archived: true, // Already archived
            },
        ];
        let config = DecayConfig::default();
        let to_archive = apply_decay(&mut memories, &config);
        
        // Should not double-archive
        assert!(to_archive.is_empty());
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let original = DecayConfig {
            decay_rate: 0.95,
            recall_boost: 0.15,
            confirm_boost: 0.25,
            reject_penalty: 0.35,
            archive_threshold: 0.15,
            max_entries: 500,
            cleanup_interval_hours: 12,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: DecayConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_config_deserialization_with_defaults() {
        // Only specify some fields, rest should be defaults
        let json = r#"{"decay_rate": 0.95}"#;
        let config: DecayConfig = serde_json::from_str(json).unwrap();
        assert!((config.decay_rate - 0.95).abs() < f64::EPSILON);
        assert!((config.recall_boost - 0.1).abs() < f64::EPSILON); // default
        assert!((config.confirm_boost - 0.2).abs() < f64::EPSILON); // default
    }
}
