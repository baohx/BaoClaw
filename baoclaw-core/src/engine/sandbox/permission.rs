//! Permission escalation flow for sandbox profile upgrades.
//!
//! When a tool tries to perform an action blocked by its current profile,
//! this module handles the escalation request, user confirmation,
//! and temporary/permanent authorization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::profile::SandboxProfile;
use super::config::SandboxConfigFile;

/// Escalation grant record.
#[derive(Clone, Debug)]
struct EscalationGrant {
    /// The tool that was granted access.
    tool: String,
    /// The action type (e.g., "network", "file_write", "full_access").
    action: String,
    /// The target resource (e.g., "github.com:443", "/etc/passwd").
    target: String,
    /// The profile used after escalation.
    profile: String,
    /// When the grant was issued.
    granted_at: Instant,
    /// How long the grant is valid. None = permanent.
    expires_at: Option<Instant>,
}

/// Permission escalation result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EscalationResult {
    /// Escalation was granted.
    Granted {
        profile: String,
        /// Whether this is a permanent or temporary grant.
        permanent: bool,
        /// The profile used after escalation (may differ from requested).
        actual_profile: String,
    },
    /// Escalation was denied by user or policy.
    Denied {
        reason: String,
    },
    /// Escalation is pending user confirmation.
    Pending {
        request_id: String,
        message: String,
    },
}

/// Permission escalation request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalationRequest {
    /// Unique ID for tracking.
    pub request_id: String,
    /// The tool making the request.
    pub tool: String,
    /// The requested action type.
    pub action: String,
    /// The target of the action.
    pub target: String,
    /// Current profile in use.
    pub current_profile: String,
    /// Desired profile after escalation.
    pub desired_profile: String,
    /// User-friendly explanation.
    pub message: String,
}

/// Manages permission escalation requests, grants, and policies.
pub struct PermissionManager {
    /// Active temporary grants.
    temp_grants: Mutex<Vec<EscalationGrant>>,
    /// Permanent grants (persisted across sessions).
    permanent_grants: Mutex<HashMap<(String, String), EscalationGrant>>,
    /// Pending confirmation requests.
    pending_requests: Mutex<HashMap<String, EscalationRequest>>,
    /// Tracks previously blocked actions for learning.
    blocked_history: Mutex<Vec<BlockedAction>>,
}

/// Record of a blocked action for pattern learning.
#[derive(Clone, Debug)]
struct BlockedAction {
    tool: String,
    action: String,
    target: String,
    profile: String,
    timestamp: Instant,
}

impl PermissionManager {
    /// Create a new permission manager.
    pub fn new() -> Self {
        Self {
            temp_grants: Mutex::new(Vec::new()),
            permanent_grants: Mutex::new(HashMap::new()),
            pending_requests: Mutex::new(HashMap::new()),
            blocked_history: Mutex::new(Vec::new()),
        }
    }

    /// Check if an action would exceed the current profile's permissions.
    /// Returns Some(request) if escalation is needed, None if allowed.
    pub fn check_escalation_needed(
        &self,
        tool: &str,
        action: &str,
        target: &str,
        profile: &SandboxProfile,
        config: &SandboxConfigFile,
    ) -> Option<EscalationRequest> {
        let needed_profile = match action {
            "network" => {
                if profile.network.is_allowed() {
                    return None;
                }
                "web_dev"
            }
            "file_write" => {
                if profile.is_writable(target) {
                    return None;
                }
                // Check if full_access is needed
                if target.starts_with("/etc/") || target.starts_with("/root/") {
                    "full_access"
                } else {
                    "web_dev"
                }
            }
            "full_access" => {
                if profile.name == "full_access" {
                    return None;
                }
                "full_access"
            }
            _ => {
                // Unknown action type — escalate to next profile
                match profile.name.as_str() {
                    "read_only" => "web_dev",
                    "web_dev" => "full_access",
                    _ => return None, // Already at max
                }
            }
        };

        let _desired = config.get_profile(needed_profile)?.clone();

        Some(EscalationRequest {
            request_id: format!("esc-{}-{}", tool, std::process::id()),
            tool: tool.to_string(),
            action: action.to_string(),
            target: target.to_string(),
            current_profile: profile.name.clone(),
            desired_profile: needed_profile.to_string(),
            message: Self::format_escalation_message(tool, action, target, &profile.name, needed_profile),
        })
    }

    /// Check if there's already an active grant for this tool+action+target.
    pub fn has_active_grant(&self, tool: &str, action: &str, target: &str) -> Option<String> {
        let key = (tool.to_string(), Self::grant_key(action, target));

        // Check permanent grants
        if let Ok(grants) = self.permanent_grants.lock() {
            if let Some(grant) = grants.get(&key) {
                return Some(grant.profile.clone());
            }
        }

        // Check temporary grants
        if let Ok(mut grants) = self.temp_grants.lock() {
            let now = Instant::now();
            grants.retain(|g| g.expires_at.is_none_or(|exp| now < exp));

            for grant in grants.iter() {
                if grant.tool == tool
                    && grant.action == action
                    && (grant.target == target || grant.target == "*")
                {
                    return Some(grant.profile.clone());
                }
            }
        }

        None
    }

    /// Request permission escalation — returns Pending if confirmation needed.
    pub fn request_escalation(
        &self,
        tool: &str,
        action: &str,
        target: &str,
        profile: &SandboxProfile,
        config: &SandboxConfigFile,
    ) -> EscalationResult {
        // Check if already granted
        if let Some(granted_profile) = self.has_active_grant(tool, action, target) {
            return EscalationResult::Granted {
                profile: granted_profile.clone(),
                permanent: false, // Temp grants are most common
                actual_profile: granted_profile,
            };
        }

        // Check if escalation is needed
        let request = match self.check_escalation_needed(tool, action, target, profile, config) {
            Some(req) => req,
            None => {
                return EscalationResult::Denied {
                    reason: "Action is already within current profile permissions".to_string(),
                };
            }
        };

        // Record the blocked action for learning
        if let Ok(mut history) = self.blocked_history.lock() {
            history.push(BlockedAction {
                tool: tool.to_string(),
                action: action.to_string(),
                target: target.to_string(),
                profile: profile.name.clone(),
                timestamp: Instant::now(),
            });
            // Keep only last 1000 entries
            if history.len() > 1000 {
                let len = history.len();
                history.drain(0..len - 1000);
            }
        }

        // Store pending request
        if let Ok(mut pending) = self.pending_requests.lock() {
            pending.insert(request.request_id.clone(), request.clone());
        }

        EscalationResult::Pending {
            request_id: request.request_id.clone(),
            message: request.message.clone(),
        }
    }

    /// Confirm a pending escalation request.
    pub fn confirm_escalation(
        &self,
        request_id: &str,
        permanent: bool,
        duration: Option<Duration>,
    ) -> EscalationResult {
        let request = match self.pending_requests.lock() {
            Ok(mut pending) => pending.remove(request_id),
            Err(e) => {
                return EscalationResult::Denied {
                    reason: format!("Lock error: {}", e),
                }
            }
        };

        let request = match request {
            Some(req) => req,
            None => {
                return EscalationResult::Denied {
                    reason: format!("No pending request with ID: {}", request_id),
                };
            }
        };

        let actual_profile = request.desired_profile.clone();

        if permanent {
            let grant = EscalationGrant {
                tool: request.tool.clone(),
                action: request.action.clone(),
                target: request.target.clone(),
                profile: actual_profile.clone(),
                granted_at: Instant::now(),
                expires_at: None,
            };

            if let Ok(mut grants) = self.permanent_grants.lock() {
                let key = (request.tool.clone(), Self::grant_key(&request.action, &request.target));
                grants.insert(key, grant);
            }

            EscalationResult::Granted {
                profile: actual_profile.clone(),
                permanent: true,
                actual_profile,
            }
        } else {
            let grant = EscalationGrant {
                tool: request.tool.clone(),
                action: request.action.clone(),
                target: request.target.clone(),
                profile: actual_profile.clone(),
                granted_at: Instant::now(),
                expires_at: duration.map(|d| Instant::now() + d).or(Some(Instant::now() + Duration::from_secs(3600))),
            };

            if let Ok(mut grants) = self.temp_grants.lock() {
                grants.push(grant);
            }

            EscalationResult::Granted {
                profile: actual_profile.clone(),
                permanent: false,
                actual_profile,
            }
        }
    }

    /// Deny a pending escalation request.
    pub fn deny_escalation(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending_requests.lock() {
            pending.remove(request_id);
        }
    }

    /// Get a pending request by ID.
    pub fn get_pending(&self, request_id: &str) -> Option<EscalationRequest> {
        self.pending_requests
            .lock()
            .ok()?
            .get(request_id)
            .cloned()
    }

    /// List all pending requests.
    pub fn list_pending(&self) -> Vec<EscalationRequest> {
        self.pending_requests
            .lock()
            .map(|pending| pending.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Revoke all temporary grants.
    pub fn revoke_temp_grants(&self) {
        if let Ok(mut grants) = self.temp_grants.lock() {
            grants.clear();
        }
    }

    /// Revoke a specific permanent grant.
    pub fn revoke_permanent(&self, tool: &str, action: &str, target: &str) -> bool {
        if let Ok(mut grants) = self.permanent_grants.lock() {
            let key = (tool.to_string(), Self::grant_key(action, target));
            grants.remove(&key).is_some()
        } else {
            false
        }
    }

    /// Get statistics about blocked actions (for learning which profiles to adjust).
    pub fn blocked_stats(&self) -> Vec<(String, String, usize)> {
        let history = self.blocked_history.lock().unwrap_or_else(|e| e.into_inner());

        let mut counts: HashMap<(String, String), usize> = HashMap::new();
        for entry in history.iter() {
            *counts.entry((entry.tool.clone(), entry.action.clone())).or_default() += 1;
        }

        let mut stats: Vec<_> = counts
            .into_iter()
            .map(|((tool, action), count)| (tool, action, count))
            .collect();
        stats.sort_by_key(|a| std::cmp::Reverse(a.2));
        stats
    }

    /// Get the number of active temporary grants.
    pub fn active_temp_grants(&self) -> usize {
        if let Ok(mut grants) = self.temp_grants.lock() {
            let now = Instant::now();
            grants.retain(|g| g.expires_at.is_none_or(|exp| now < exp));
            grants.len()
        } else {
            0
        }
    }

    // ── helpers ──

    fn format_escalation_message(tool: &str, action: &str, target: &str, current: &str, desired: &str) -> String {
        format!(
            "⚠️  {} wants to perform '{}' on '{}', but current profile '{}' does not allow it.\n \
             Requested escalation to '{}' profile.",
            tool, action, target, current, desired
        )
    }

    fn grant_key(action: &str, target: &str) -> String {
        format!("{}:{}", action, target)
    }
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::SandboxConfigFile;
    use super::super::profile::SandboxProfile;

    #[test]
    fn test_escalation_needed_for_network() {
        let manager = PermissionManager::new();
        let profile = SandboxProfile::read_only();
        let config = SandboxConfigFile::default();

        let request = manager.check_escalation_needed(
            "Bash",
            "network",
            "npmjs.org",
            &profile,
            &config,
        );
        assert!(request.is_some());
        let req = request.unwrap();
        assert_eq!(req.desired_profile, "web_dev");
        assert_eq!(req.current_profile, "read_only");
    }

    #[test]
    fn test_no_escalation_when_allowed() {
        let manager = PermissionManager::new();
        let profile = SandboxProfile::web_dev();
        let config = SandboxConfigFile::default();

        // web_dev allows npmjs.org
        let result = manager.request_escalation(
            "Bash",
            "network",
            "npmjs.org",
            &profile,
            &config,
        );
        match result {
            EscalationResult::Denied { reason } => {
                assert!(reason.contains("within current profile"));
            }
            _ => panic!("Expected Denied, got {:?}", result),
        }
    }

    #[test]
    fn test_request_and_confirm_escalation() {
        let manager = PermissionManager::new();
        let profile = SandboxProfile::read_only();
        let config = SandboxConfigFile::default();

        // First request — should be pending
        let result = manager.request_escalation(
            "Bash",
            "network",
            "npmjs.org",
            &profile,
            &config,
        );

        let request_id = match result {
            EscalationResult::Pending { request_id, .. } => request_id,
            _ => panic!("Expected Pending, got {:?}", result),
        };

        // Can deny first — just clears pending
        manager.deny_escalation(&request_id);
        assert!(manager.get_pending(&request_id).is_none());

        // Request again for confirmation test
        let result2 = manager.request_escalation(
            "Bash",
            "network",
            "npmjs.org",
            &profile,
            &config,
        );
        let request_id2 = match result2 {
            EscalationResult::Pending { request_id, .. } => request_id,
            _ => panic!("Expected Pending"),
        };

        // Confirm the escalation
        let result3 = manager.confirm_escalation(&request_id2, true, None);
        match result3 {
            EscalationResult::Granted { permanent, actual_profile, .. } => {
                assert!(permanent);
                assert_eq!(actual_profile, "web_dev");
            }
            _ => panic!("Expected Granted"),
        }

        // Now the same action should be auto-granted
        let result4 = manager.request_escalation(
            "Bash",
            "network",
            "npmjs.org",
            &profile,
            &config,
        );
        match result4 {
            EscalationResult::Granted { profile: p, .. } => {
                assert_eq!(p, "web_dev");
            }
            _ => panic!("Expected auto-Granted"),
        }
    }

    #[test]
    fn test_temporary_grant_expires() {
        let manager = PermissionManager::new();
        let profile = SandboxProfile::read_only();
        let config = SandboxConfigFile::default();

        let result = manager.request_escalation(
            "FileWrite",
            "file_write",
            "src/main.rs",
            &profile,
            &config,
        );

        let request_id = match result {
            EscalationResult::Pending { request_id, .. } => request_id,
            _ => panic!("Expected Pending"),
        };

        // Grant with longer duration to survive test execution latency under tarpaulin
        manager.confirm_escalation(&request_id, false, Some(Duration::from_millis(500)));

        // Should still have active grant immediately
        assert!(manager.has_active_grant("FileWrite", "file_write", "src/main.rs").is_some());

        // Wait for grant to expire
        std::thread::sleep(Duration::from_millis(600));

        // Grant should have expired
        assert!(manager.has_active_grant("FileWrite", "file_write", "src/main.rs").is_none());
    }

    #[test]
    fn test_escalation_to_full_access() {
        let manager = PermissionManager::new();
        let profile = SandboxProfile::web_dev();
        let config = SandboxConfigFile::default();

        let request = manager.check_escalation_needed(
            "FileWrite",
            "file_write",
            "/etc/hosts",
            &profile,
            &config,
        );
        assert!(request.is_some());
        let req = request.unwrap();
        assert_eq!(req.desired_profile, "full_access");
    }

    #[test]
    fn test_revoke_permanent_grant() {
        let manager = PermissionManager::new();
        let profile = SandboxProfile::read_only();
        let config = SandboxConfigFile::default();

        // Create a permanent grant
        let result = manager.request_escalation(
            "Bash",
            "permanent_op",
            "/some/path",
            &profile,
            &config,
        );
        let request_id = match result {
            EscalationResult::Pending { request_id, .. } => request_id,
            _ => panic!("Expected Pending"),
        };
        manager.confirm_escalation(&request_id, true, None);

        // Should be granted
        assert!(manager.has_active_grant("Bash", "permanent_op", "/some/path").is_some());

        // Revoke
        assert!(manager.revoke_permanent("Bash", "permanent_op", "/some/path"));

        // Should no longer be granted
        assert!(manager.has_active_grant("Bash", "permanent_op", "/some/path").is_none());
    }

    #[test]
    fn test_blocked_stats() {
        let manager = PermissionManager::new();
        let profile = SandboxProfile::read_only();
        let config = SandboxConfigFile::default();

        // Generate some blocked actions
        for _ in 0..3 {
            manager.request_escalation("Bash", "network", "google.com", &profile, &config);
        }
        for _ in 0..2 {
            manager.request_escalation("FileWrite", "file_write", "/test", &profile, &config);
        }

        let stats = manager.blocked_stats();
        assert!(stats.len() >= 2);
        // Bash+network should have the most blocks
        assert_eq!(stats[0], ("Bash".to_string(), "network".to_string(), 3));
    }
}
