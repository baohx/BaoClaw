use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::query_engine::{EngineEvent, QueryEngine};

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
}

impl SessionRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
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
}
