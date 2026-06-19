use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

use chrono::Utc;

use super::query_engine::{EngineEvent, QueryEngine};
use super::session_persistence::{
    self, PersistedSession,
};

/// Unique identifier for a client connection within a shared session.
pub type ClientId = u64;

/// A shared session wrapping a QueryEngine for multi-client access.
///
/// Provides concurrency control via an ActiveSubmitter lock and
/// event broadcasting to all connected clients.
pub struct SharedSession {
    /// The shared QueryEngine, protected by RwLock for multi-read / single-write.
    engine: Arc<RwLock<QueryEngine>>,
    /// The currently active message submitter (at most one at a time).
    active_submitter: Mutex<Option<ClientId>>,
    /// Broadcast sender for engine events.
    event_tx: broadcast::Sender<EngineEvent>,
    /// Set of currently connected client IDs.
    connected_clients: Mutex<HashSet<ClientId>>,
    /// Monotonic counter for generating unique client IDs.
    next_client_id: AtomicU64,
}

impl SharedSession {
    /// Create a new SharedSession wrapping the given QueryEngine.
    pub fn new(engine: QueryEngine, broadcast_capacity: usize) -> Self {
        let (event_tx, _) = broadcast::channel(broadcast_capacity);
        Self {
            engine: Arc::new(RwLock::new(engine)),
            active_submitter: Mutex::new(None),
            event_tx,
            connected_clients: Mutex::new(HashSet::new()),
            next_client_id: AtomicU64::new(1),
        }
    }

    /// Register a new client. Returns the assigned ClientId and a broadcast receiver.
    pub async fn add_client(&self) -> (ClientId, broadcast::Receiver<EngineEvent>) {
        let id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        self.connected_clients.lock().await.insert(id);
        let rx = self.event_tx.subscribe();
        (id, rx)
    }

    /// Remove a client from the session.
    ///
    /// If the removed client held the ActiveSubmitter lock, it is automatically released.
    /// Returns `true` if this was the last connected client (session should be cleaned up).
    pub async fn remove_client(&self, client_id: ClientId) -> bool {
        self.connected_clients.lock().await.remove(&client_id);

        // Auto-release ActiveSubmitter if held by this client
        let mut submitter = self.active_submitter.lock().await;
        if *submitter == Some(client_id) {
            *submitter = None;
        }

        self.connected_clients.lock().await.is_empty()
    }

    /// Try to acquire the ActiveSubmitter lock for the given client.
    ///
    /// Returns `true` if the lock was acquired, `false` if another client already holds it.
    pub async fn try_acquire_submitter(&self, client_id: ClientId) -> bool {
        let mut submitter = self.active_submitter.lock().await;
        if submitter.is_none() {
            *submitter = Some(client_id);
            true
        } else {
            false
        }
    }

    /// Release the ActiveSubmitter lock if held by the given client.
    pub async fn release_submitter(&self, client_id: ClientId) {
        let mut submitter = self.active_submitter.lock().await;
        if *submitter == Some(client_id) {
            *submitter = None;
        }
    }

    /// Acquire a read lock on the shared QueryEngine.
    pub async fn engine_read(&self) -> RwLockReadGuard<'_, QueryEngine> {
        self.engine.read().await
    }

    /// Acquire a write lock on the shared QueryEngine.
    pub async fn engine_write(&self) -> RwLockWriteGuard<'_, QueryEngine> {
        self.engine.write().await
    }

    /// Broadcast an event to all connected clients.
    ///
    /// Errors from lagged receivers are silently ignored.
    pub fn broadcast(&self, event: EngineEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Get the number of currently connected clients.
    pub async fn client_count(&self) -> usize {
        self.connected_clients.lock().await.len()
    }

    /// Check whether the given client is the current ActiveSubmitter.
    pub async fn is_active_submitter(&self, client_id: ClientId) -> bool {
        *self.active_submitter.lock().await == Some(client_id)
    }

    /// Check whether there is any active submitter.
    pub async fn has_active_submitter(&self) -> bool {
        self.active_submitter.lock().await.is_some()
    }
}

/// Global registry of shared sessions, keyed by session ID.
pub struct SessionRegistry {
    sessions: Mutex<HashMap<String, Arc<SharedSession>>>,
    /// Directory for session persistence files (~/.baoclaw/sessions/).
    persistence_dir: PathBuf,
}

impl SessionRegistry {
    /// Create an empty registry with default persistence directory.
    pub fn new() -> Self {
        Self::with_persistence_dir(session_persistence::default_sessions_dir())
    }

    /// Create an empty registry with a custom persistence directory.
    /// Ensures the directory exists.
    pub fn with_persistence_dir(persistence_dir: PathBuf) -> Self {
        // Create directory if it doesn't exist (backward compatible: no error on first run)
        if let Err(e) = std::fs::create_dir_all(&persistence_dir) {
            eprintln!(
                "[session-registry] WARNING: failed to create persistence dir {:?}: {}",
                persistence_dir, e
            );
        }
        Self {
            sessions: Mutex::new(HashMap::new()),
            persistence_dir,
        }
    }

    /// Get the persistence directory.
    pub fn persistence_dir(&self) -> &PathBuf {
        &self.persistence_dir
    }

    /// Look up or create a shared session.
    ///
    /// `config_factory` is called only when a new session needs to be created.
    /// Returns `(session, is_new)` where `is_new` is `true` if a new session was created.
    pub async fn get_or_create(
        &self,
        session_id: &str,
        config_factory: impl FnOnce() -> QueryEngine,
    ) -> (Arc<SharedSession>, bool) {
        let mut sessions = self.sessions.lock().await;
        if let Some(existing) = sessions.get(session_id) {
            (Arc::clone(existing), false)
        } else {
            let engine = config_factory();
            let session = Arc::new(SharedSession::new(engine, 256));
            sessions.insert(session_id.to_string(), Arc::clone(&session));
            (session, true)
        }
    }

    /// Remove a session from the registry.
    pub async fn remove(&self, session_id: &str) {
        self.sessions.lock().await.remove(session_id);
    }

    /// Check whether a session exists in the registry.
    pub async fn contains(&self, session_id: &str) -> bool {
        self.sessions.lock().await.contains_key(session_id)
    }

    // ── Persistence Methods ──

    /// Persist a session's conversation state to disk (called after each turn).
    ///
    /// Serializes messages + metadata to `<session_id>.json` using atomic write.
    /// Also updates the registry index's `last_active` timestamp.
    ///
    /// Errors are logged but do not crash the daemon. Returns `Ok(())` on success.
    pub async fn persist_session(&self, session_id: &str) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        let session = match sessions.get(session_id) {
            Some(s) => Arc::clone(s),
            None => return Err(format!("session {} not found in registry", session_id)),
        };
        drop(sessions); // release lock before acquiring engine read lock

        let engine = session.engine_read().await;
        let messages = engine.get_messages().to_vec();
        let cwd = engine.get_cwd().to_string_lossy().to_string();
        let model = engine.get_model().to_string();
        let memory_summary = engine.get_session_memory().as_ref().map(|sm| sm.get());
        drop(engine);

        let now = Utc::now().to_rfc3339();
        // Use created_at from existing registry entry, or now for new sessions
        let registry = session_persistence::load_registry(&self.persistence_dir);
        let created_at = registry
            .sessions
            .iter()
            .find(|e| e.session_id == session_id)
            .map(|e| e.created_at.clone())
            .unwrap_or_else(|| now.clone());

        let state = PersistedSession {
            session_id: session_id.to_string(),
            cwd,
            model,
            created_at: created_at.clone(),
            last_active: now,
            messages,
            memory_summary,
        };

        session_persistence::persist_session_state(&self.persistence_dir, &state)
            .map_err(|e| {
                let msg = format!("failed to persist session {}: {}", session_id, e);
                eprintln!("[session-registry] WARNING: {}", msg);
                msg
            })
    }

    /// Load persisted session data for a given session ID from disk.
    ///
    /// Returns `None` if the session has no persisted state or if the file is corrupted.
    /// This is used by the daemon to restore conversation history when a session is
    /// re-created after a crash or restart.
    pub fn load_persisted_session(&self, session_id: &str) -> Option<PersistedSession> {
        session_persistence::load_session_state(&self.persistence_dir, session_id)
    }

    /// Restore messages into a session from persisted state (if available).
    ///
    /// Should be called right after `get_or_create` when `is_new` is true.
    /// Returns `true` if messages were restored, `false` otherwise.
    pub async fn restore_session_messages(&self, session_id: &str) -> bool {
        let persisted = match self.load_persisted_session(session_id) {
            Some(p) => p,
            None => return false,
        };

        if persisted.messages.is_empty() {
            return false;
        }

        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(session_id) {
            let mut engine = session.engine_write().await;
            engine.set_messages(persisted.messages);

            // Restore memory summary
            if let Some(summary) = persisted.memory_summary {
                if !summary.is_empty() {
                    engine.seed_session_memory(&summary);
                }
            }
            true
        } else {
            false
        }
    }

    /// Archive sessions that have been inactive for more than `max_age_days`.
    ///
    /// Moves their JSON files to `archive/` subdirectory and removes them
    /// from the registry index. In-memory sessions are not evicted (use `remove()`
    /// for that).
    ///
    /// Returns the list of archived session IDs.
    pub fn archive_stale(&self, max_age_days: u64) -> Vec<String> {
        match session_persistence::archive_stale_sessions(&self.persistence_dir, max_age_days) {
            Ok(archived) => {
                if !archived.is_empty() {
                    eprintln!(
                        "[session-registry] Archived {} stale sessions (>{} days inactive)",
                        archived.len(),
                        max_age_days
                    );
                }
                archived
            }
            Err(e) => {
                eprintln!(
                    "[session-registry] WARNING: archive_stale failed: {}",
                    e
                );
                Vec::new()
            }
        }
    }

    /// Archive sessions with the default 7-day threshold.
    pub fn archive_stale_default(&self) -> Vec<String> {
        self.archive_stale(7)
    }

    /// Get all session IDs from the registry index (on disk).
    pub fn list_persisted_sessions(&self) -> Vec<(String, String)> {
        let index = session_persistence::load_registry(&self.persistence_dir);
        index
            .sessions
            .iter()
            .map(|e| (e.session_id.clone(), e.cwd.clone()))
            .collect()
    }

    /// Delete a session's persisted state from disk.
    pub fn delete_persisted_session(&self, session_id: &str) {
        if let Err(e) = session_persistence::delete_session(&self.persistence_dir, session_id) {
            eprintln!(
                "[session-registry] WARNING: failed to delete persisted session {}: {}",
                session_id, e
            );
        }
    }

    /// Persist all currently in-memory sessions to disk.
    /// Useful for graceful shutdown (SIGTERM/SIGINT handlers).
    pub async fn persist_all(&self) {
        let session_ids: Vec<String> = {
            let sessions = self.sessions.lock().await;
            sessions.keys().cloned().collect()
        };

        for sid in &session_ids {
            if let Err(e) = self.persist_session(sid).await {
                eprintln!(
                    "[session-registry] WARNING: persist_all failed for {}: {}",
                    sid, e
                );
            }
        }
        eprintln!(
            "[session-registry] Persisted {} sessions to disk",
            session_ids.len()
        );
    }
}
