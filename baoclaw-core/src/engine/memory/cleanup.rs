//! Memory cleanup scheduler for periodic maintenance.
//!
//! This module implements a background task that runs memory maintenance
//! at configurable intervals (default: every 24 hours). The cleanup:
//! 1. Applies decay to all memories based on time since last recall
//! 2. Archives memories below the importance threshold
//! 3. Cleans up the archive when it exceeds max_entries
//!
//! The scheduler runs asynchronously without blocking the main flow.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::engine::memory::{DecayConfig, MemoryArchive, MemoryStore};

// DEFAULT_CLEANUP_INTERVAL_HOURS is now configured via DecayConfig.cleanup_interval_hours

/// Result of a cleanup run.
#[derive(Debug, Clone)]
pub struct CleanupResult {
    /// Number of memories archived.
    pub archived_count: usize,
    /// Number of memories permanently deleted from archive.
    pub deleted_count: usize,
    /// Timestamp of the cleanup run.
    pub timestamp: String,
    /// Duration of the cleanup run in milliseconds.
    pub duration_ms: u64,
}

/// State for tracking cleanup runs.
#[derive(Debug, Clone, Default)]
pub struct CleanupState {
    /// Last cleanup timestamp (ISO8601).
    pub last_run: Option<String>,
    /// Number of cleanup runs since startup.
    pub run_count: u32,
    /// Total memories archived across all runs.
    pub total_archived: usize,
    /// Total memories deleted across all runs.
    pub total_deleted: usize,
}

/// Memory cleanup scheduler.
///
/// Runs periodic maintenance on the memory store to decay old memories
/// and archive low-importance ones. Runs in a background tokio task.
pub struct MemoryCleanupScheduler {
    /// Memory store to maintain.
    memory_store: Arc<MemoryStore>,
    /// Memory archive for archived memories.
    archive: Arc<MemoryArchive>,
    /// Decay configuration.
    config: DecayConfig,
    /// Cleanup interval in seconds.
    interval_secs: u64,
    /// Cleanup state tracking.
    state: Mutex<CleanupState>,
    /// Flag to signal shutdown.
    shutdown: Mutex<bool>,
}

impl MemoryCleanupScheduler {
    /// Create a new cleanup scheduler.
    ///
    /// # Arguments
    /// * `memory_store` - The memory store to maintain
    /// * `archive` - The archive for low-importance memories
    /// * `config` - Decay configuration (cleanup_interval_hours is used)
    pub fn new(
        memory_store: Arc<MemoryStore>,
        archive: Arc<MemoryArchive>,
        config: DecayConfig,
    ) -> Self {
        let interval_secs = config.cleanup_interval_hours * 3600;
        Self {
            memory_store,
            archive,
            config,
            interval_secs,
            state: Mutex::new(CleanupState::default()),
            shutdown: Mutex::new(false),
        }
    }

    /// Create with custom interval (for testing).
    pub fn with_interval(
        memory_store: Arc<MemoryStore>,
        archive: Arc<MemoryArchive>,
        config: DecayConfig,
        interval_secs: u64,
    ) -> Self {
        Self {
            memory_store,
            archive,
            config,
            interval_secs,
            state: Mutex::new(CleanupState::default()),
            shutdown: Mutex::new(false),
        }
    }

    /// Run a single cleanup cycle.
    ///
    /// This is the main maintenance function that:
    /// 1. Applies decay to all memories
    /// 2. Archives low-importance memories
    /// 3. Cleans up archive if needed
    ///
    /// Returns the cleanup result.
    pub async fn run_cleanup(&self) -> CleanupResult {
        let start = std::time::Instant::now();
        let timestamp = chrono::Utc::now().to_rfc3339();

        eprintln!("Memory cleanup: starting maintenance run...");

        // Run maintenance on memory store
        let result = self
            .memory_store
            .run_maintenance(&self.archive, &self.config)
            .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        eprintln!(
            "Memory cleanup: complete ({} archived, {} deleted, {}ms)",
            result.archived_ids.len(),
            result.deleted_count,
            duration_ms
        );

        // Update state
        {
            let mut state = self.state.lock().await;
            state.last_run = Some(timestamp.clone());
            state.run_count += 1;
            state.total_archived += result.archived_ids.len();
            state.total_deleted += result.deleted_count;
        }

        CleanupResult {
            archived_count: result.archived_ids.len(),
            deleted_count: result.deleted_count,
            timestamp,
            duration_ms,
        }
    }

    /// Get current cleanup state.
    pub async fn get_state(&self) -> CleanupState {
        self.state.lock().await.clone()
    }

    /// Check if a cleanup is due based on time since last run.
    pub async fn is_cleanup_due(&self) -> bool {
        let state = self.state.lock().await;
        match &state.last_run {
            None => true,
            Some(last) => {
                let last_time = chrono::DateTime::parse_from_rfc3339(last)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now() - chrono::Duration::days(365));
                let elapsed = (chrono::Utc::now() - last_time).num_seconds() as u64;
                elapsed >= self.interval_secs
            }
        }
    }

    /// Signal the scheduler to stop.
    pub async fn shutdown(&self) {
        let mut shutdown = self.shutdown.lock().await;
        *shutdown = true;
    }

    /// Check if shutdown was requested.
    async fn is_shutdown(&self) -> bool {
        *self.shutdown.lock().await
    }

    /// Start the scheduler loop.
    ///
    /// Runs forever (or until shutdown), checking periodically if a cleanup
    /// is needed. Does not block the caller - spawns a background task.
    ///
    /// # Returns
    /// A handle to the background task. The task will run until shutdown.
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            eprintln!(
                "Memory cleanup scheduler started (interval: {}h)",
                self.interval_secs / 3600
            );

            loop {
                // Check for shutdown
                if self.is_shutdown().await {
                    eprintln!("Memory cleanup scheduler: shutting down");
                    break;
                }

                // Check if cleanup is due
                if self.is_cleanup_due().await {
                    let result = self.run_cleanup().await;
                    eprintln!(
                        "Memory cleanup: archived {}, deleted {}, took {}ms",
                        result.archived_count, result.deleted_count, result.duration_ms
                    );
                }

                // Sleep for a check interval (5 minutes)
                // We check more frequently than the cleanup interval to allow
                // for precise timing and quick shutdown response
                tokio::time::sleep(Duration::from_secs(300)).await;
            }

            eprintln!("Memory cleanup scheduler stopped");
        })
    }

    /// Run cleanup immediately (for manual trigger).
    ///
    /// This bypasses the interval check and runs maintenance now.
    pub async fn run_now(&self) -> CleanupResult {
        self.run_cleanup().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> DecayConfig {
        DecayConfig {
            decay_rate: 0.99,
            recall_boost: 0.1,
            confirm_boost: 0.2,
            reject_penalty: 0.3,
            archive_threshold: 0.1,
            max_entries: 1000,
            cleanup_interval_hours: 24,
        }
    }

    #[tokio::test]
    async fn test_scheduler_creation() {
        let store = Arc::new(MemoryStore::load());
        let archive = Arc::new(MemoryArchive::load());
        let config = create_test_config();

        let scheduler =
            MemoryCleanupScheduler::new(Arc::clone(&store), Arc::clone(&archive), config);

        // Should be due for first run
        assert!(scheduler.is_cleanup_due().await);
    }

    #[tokio::test]
    async fn test_run_cleanup() {
        let store = Arc::new(MemoryStore::load());
        let archive = Arc::new(MemoryArchive::load());
        let config = create_test_config();

        // Clear for test
        store.clear().await.unwrap();
        archive.clear().await;

        let scheduler =
            MemoryCleanupScheduler::new(Arc::clone(&store), Arc::clone(&archive), config);

        let result = scheduler.run_cleanup().await;

        // Should complete without error
        assert_eq!(result.archived_count, 0); // Empty store
        assert_eq!(result.deleted_count, 0);
        assert!(!result.timestamp.is_empty());
    }

    #[tokio::test]
    async fn test_state_tracking() {
        let store = Arc::new(MemoryStore::load());
        let archive = Arc::new(MemoryArchive::load());
        let config = create_test_config();

        store.clear().await.unwrap();
        archive.clear().await;

        let scheduler =
            MemoryCleanupScheduler::new(Arc::clone(&store), Arc::clone(&archive), config);

        // Initial state
        let state = scheduler.get_state().await;
        assert_eq!(state.run_count, 0);
        assert!(state.last_run.is_none());

        // After one run
        scheduler.run_cleanup().await;
        let state = scheduler.get_state().await;
        assert_eq!(state.run_count, 1);
        assert!(state.last_run.is_some());

        // After second run
        scheduler.run_cleanup().await;
        let state = scheduler.get_state().await;
        assert_eq!(state.run_count, 2);
    }

    #[tokio::test]
    async fn test_not_due_after_run() {
        let store = Arc::new(MemoryStore::load());
        let archive = Arc::new(MemoryArchive::load());
        let config = create_test_config();

        store.clear().await.unwrap();
        archive.clear().await;

        let scheduler =
            MemoryCleanupScheduler::new(Arc::clone(&store), Arc::clone(&archive), config);

        // Initially due
        assert!(scheduler.is_cleanup_due().await);

        // After run, not due anymore (within interval)
        scheduler.run_cleanup().await;
        assert!(!scheduler.is_cleanup_due().await);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let store = Arc::new(MemoryStore::load());
        let archive = Arc::new(MemoryArchive::load());
        let config = create_test_config();

        let scheduler = Arc::new(MemoryCleanupScheduler::new(
            Arc::clone(&store),
            Arc::clone(&archive),
            config,
        ));

        // Request shutdown
        scheduler.shutdown().await;

        // Should be flagged
        assert!(scheduler.is_shutdown().await);
    }

    #[tokio::test]
    async fn test_custom_interval() {
        let store = Arc::new(MemoryStore::load());
        let archive = Arc::new(MemoryArchive::load());
        let config = create_test_config();

        // Use very short interval for testing
        let scheduler = MemoryCleanupScheduler::with_interval(
            Arc::clone(&store),
            Arc::clone(&archive),
            config,
            1, // 1 second
        );

        // Should be due immediately
        assert!(scheduler.is_cleanup_due().await);

        // Run cleanup
        scheduler.run_cleanup().await;

        // Still due because interval is very short
        // (but the timestamp check has 1-second resolution)
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(scheduler.is_cleanup_due().await);
    }
}
