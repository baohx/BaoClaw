//! Thread-safe permission cache for storing granted decisions.
//!
//! The cache stores temporary and permanent grants that allow the
//! PermissionGate to skip user prompts for previously-authorized actions.
//! Uses a Mutex-protected Vec for thread safety.

use std::sync::Mutex;

use super::types::{CacheEntry, DecisionType};

/// Thread-safe cache of permission grants.
///
/// Stores decisions of type `AllowSession` and `AllowPermanent`
/// that were made by the user. Entries are checked before
/// rules are evaluated, so cached grants take priority.
pub struct PermissionCache {
    /// The underlying cache entries, protected by a Mutex.
    entries: Mutex<Vec<CacheEntry>>,
}

impl PermissionCache {
    /// Create a new empty permission cache.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Check if there is a valid (non-expired) grant for the given tool, action, and target.
    ///
    /// Returns `Some(DecisionType)` if a matching cached grant exists and has not expired.
    /// Returns `None` if no matching grant is found or all matches have expired.
    pub fn check(&self, tool: &str, action: &str, target: &str) -> Option<DecisionType> {
        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        // First pass: find matching entries
        let mut found = false;
        let mut decision = DecisionType::Deny;

        for entry in entries.iter() {
            if entry.matches(tool, action, target) && !entry.is_expired() {
                found = true;
                decision = entry.grant.clone();
                break;
            }
        }

        if found {
            return Some(decision);
        }

        // No valid matching entry found — clean up expired entries as a side effect
        entries.retain(|e| !e.is_expired());
        None
    }

    /// Store a cache entry for future permission checks.
    ///
    /// # Arguments
    /// * `tool` - The tool name.
    /// * `action` - The action/command.
    /// * `target` - The target resource.
    /// * `decision` - The decision type (only AllowSession and AllowPermanent are cached).
    /// * `ttl_secs` - Optional time-to-live in seconds. `None` means permanent.
    pub fn store(
        &self,
        tool: &str,
        action: &str,
        target: &str,
        decision: DecisionType,
        ttl_secs: Option<u64>,
    ) {
        // Only cache session-level and permanent grants
        if !matches!(decision, DecisionType::AllowSession | DecisionType::AllowPermanent) {
            return;
        }

        let entry = CacheEntry::new(decision, tool, action, target, ttl_secs);

        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        // Remove any existing entries for the same tool/action/target
        entries.retain(|e| !e.matches(tool, action, target));

        entries.push(entry);
    }

    /// Clean up all expired entries from the cache.
    pub fn cleanup(&self) {
        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        entries.retain(|e| !e.is_expired());
    }

    /// Clear all entries from the cache, regardless of expiry.
    pub fn clear(&self) {
        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        entries.clear();
    }

    /// Return the number of entries currently in the cache (including expired).
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|e| e.len())
            .unwrap_or(0)
    }

    /// Return true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Revoke a specific cached grant matching tool, action, and target.
    /// Returns the number of entries removed.
    pub fn revoke(&self, tool: &str, action: &str, target: &str) -> usize {
        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let before = entries.len();
        entries.retain(|e| !e.matches(tool, action, target));
        before - entries.len()
    }
}

impl Default for PermissionCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::UNIX_EPOCH;
    use std::time::SystemTime;
    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    #[test]
    fn test_cache_store_and_check() {
        let cache = PermissionCache::new();

        // Store a session grant
        cache.store(
            "Bash",
            "git status",
            ".",
            DecisionType::AllowSession,
            Some(3600),
        );

        // Should find the grant
        let result = cache.check("Bash", "git status", ".");
        assert_eq!(result, Some(DecisionType::AllowSession));

        // Different action should not match
        let result = cache.check("Bash", "git push", ".");
        assert_eq!(result, None);
    }

    #[test]
    fn test_cache_permanent_grant() {
        let cache = PermissionCache::new();

        cache.store(
            "FileRead",
            "read",
            "src/main.rs",
            DecisionType::AllowPermanent,
            None, // Permanent
        );

        let result = cache.check("FileRead", "read", "src/main.rs");
        assert_eq!(result, Some(DecisionType::AllowPermanent));
    }

    #[test]
    fn test_cache_expired_entry_not_found() {
        let cache = PermissionCache::new();

        // Manually insert an expired entry
        let expired_entry = CacheEntry {
            grant: DecisionType::AllowSession,
            tool: "Bash".to_string(),
            action: "ls".to_string(),
            target: "/tmp".to_string(),
            expires_at: Some(0), // Already expired
        };

        {
            let mut entries = cache.entries.lock().unwrap();
            entries.push(expired_entry);
        }

        // Should not find the expired entry
        let result = cache.check("Bash", "ls", "/tmp");
        assert_eq!(result, None);

        // cleanup should remove it
        cache.cleanup();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_clear() {
        let cache = PermissionCache::new();

        cache.store("Bash", "cmd1", "/a", DecisionType::AllowSession, Some(3600));
        cache.store("Bash", "cmd2", "/b", DecisionType::AllowPermanent, None);

        assert_eq!(cache.len(), 2);
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_revoke() {
        let cache = PermissionCache::new();

        cache.store("Bash", "npm install", "node_modules", DecisionType::AllowSession, Some(3600));
        cache.store("Bash", "npm test", "node_modules", DecisionType::AllowSession, Some(3600));

        assert_eq!(cache.len(), 2);

        let removed = cache.revoke("Bash", "npm install", "node_modules");
        assert_eq!(removed, 1);
        assert_eq!(cache.len(), 1);

        // Should still find the other entry
        let result = cache.check("Bash", "npm test", "node_modules");
        assert_eq!(result, Some(DecisionType::AllowSession));
    }

    #[test]
    fn test_cache_overwrite_existing() {
        let cache = PermissionCache::new();

        // Store first grant
        cache.store(
            "Bash",
            "deploy",
            "prod",
            DecisionType::AllowSession,
            Some(3600),
        );

        // Store a different grant for the same key
        cache.store(
            "Bash",
            "deploy",
            "prod",
            DecisionType::AllowPermanent,
            None,
        );

        // Should return the latest grant type
        let result = cache.check("Bash", "deploy", "prod");
        assert_eq!(result, Some(DecisionType::AllowPermanent));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_allow_once_not_cached() {
        let cache = PermissionCache::new();

        // AllowOnce should not be stored in cache
        cache.store(
            "Bash",
            "dangerous-op",
            "/tmp",
            DecisionType::AllowOnce,
            None,
        );

        assert!(cache.is_empty());
    }
}
