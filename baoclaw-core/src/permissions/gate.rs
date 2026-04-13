use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::oneshot;

/// User's permission decision sent from CLI
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum PermissionDecision {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "allow_always")]
    AllowAlways {
        #[serde(skip_serializing_if = "Option::is_none")]
        rule: Option<String>,
    },
}

/// Permission decision channel — maintained in QueryEngine,
/// used by ToolExecutor to wait for user responses.
#[derive(Clone)]
pub struct PermissionGate {
    pending: Arc<RwLock<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
}

impl PermissionGate {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a pending permission request, returns a receiver to await the decision.
    pub fn request(&self, tool_use_id: &str) -> oneshot::Receiver<PermissionDecision> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .write()
            .unwrap()
            .insert(tool_use_id.to_string(), tx);
        rx
    }

    /// Submit a user's permission decision. Returns true if the decision was delivered.
    pub fn respond(&self, tool_use_id: &str, decision: PermissionDecision) -> bool {
        if let Some(tx) = self.pending.write().unwrap().remove(tool_use_id) {
            tx.send(decision).is_ok()
        } else {
            false
        }
    }

    /// Returns the number of pending permission requests.
    pub fn pending_count(&self) -> usize {
        self.pending.read().unwrap().len()
    }
}

impl Default for PermissionGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_request_respond_pair() {
        let gate = PermissionGate::new();
        let rx = gate.request("tu_001");
        assert_eq!(gate.pending_count(), 1);

        let sent = gate.respond("tu_001", PermissionDecision::Allow);
        assert!(sent);
        assert_eq!(gate.pending_count(), 0);

        let decision = rx.await.unwrap();
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn test_respond_deny() {
        let gate = PermissionGate::new();
        let rx = gate.request("tu_002");

        gate.respond("tu_002", PermissionDecision::Deny);
        let decision = rx.await.unwrap();
        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[tokio::test]
    async fn test_respond_allow_always() {
        let gate = PermissionGate::new();
        let rx = gate.request("tu_003");

        gate.respond(
            "tu_003",
            PermissionDecision::AllowAlways {
                rule: Some("Bash(git *)".to_string()),
            },
        );
        let decision = rx.await.unwrap();
        match decision {
            PermissionDecision::AllowAlways { rule } => {
                assert_eq!(rule, Some("Bash(git *)".to_string()));
            }
            _ => panic!("Expected AllowAlways"),
        }
    }

    #[tokio::test]
    async fn test_respond_unknown_id_returns_false() {
        let gate = PermissionGate::new();
        let sent = gate.respond("nonexistent", PermissionDecision::Allow);
        assert!(!sent);
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let gate = PermissionGate::new();
        let rx1 = gate.request("tu_a");
        let rx2 = gate.request("tu_b");
        let rx3 = gate.request("tu_c");
        assert_eq!(gate.pending_count(), 3);

        gate.respond("tu_b", PermissionDecision::Deny);
        gate.respond("tu_a", PermissionDecision::Allow);
        gate.respond(
            "tu_c",
            PermissionDecision::AllowAlways { rule: None },
        );

        assert_eq!(rx1.await.unwrap(), PermissionDecision::Allow);
        assert_eq!(rx2.await.unwrap(), PermissionDecision::Deny);
        assert_eq!(
            rx3.await.unwrap(),
            PermissionDecision::AllowAlways { rule: None }
        );
        assert_eq!(gate.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_dropped_receiver_respond_returns_false() {
        let gate = PermissionGate::new();
        let rx = gate.request("tu_drop");
        drop(rx); // receiver dropped before respond

        let sent = gate.respond("tu_drop", PermissionDecision::Allow);
        assert!(!sent); // send fails because receiver is gone
    }

    #[test]
    fn test_clone_shares_state() {
        let gate = PermissionGate::new();
        let gate2 = gate.clone();
        let _rx = gate.request("tu_shared");
        assert_eq!(gate2.pending_count(), 1);
    }

    #[test]
    fn test_default_creates_empty_gate() {
        let gate = PermissionGate::default();
        assert_eq!(gate.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_timeout_auto_deny() {
        let gate = PermissionGate::new();
        let rx = gate.request("tu_timeout");

        // Use a very short timeout to simulate the 5-minute timeout behavior
        let decision = match tokio::time::timeout(
            std::time::Duration::from_millis(10),
            rx,
        )
        .await
        {
            Ok(Ok(d)) => d,
            Ok(Err(_)) => PermissionDecision::Deny,
            Err(_) => PermissionDecision::Deny, // timeout → auto-deny
        };

        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[tokio::test]
    async fn test_multiple_respond_same_id_second_fails() {
        let gate = PermissionGate::new();
        let rx = gate.request("tu_dup");

        let first = gate.respond("tu_dup", PermissionDecision::Allow);
        assert!(first);

        // Second respond for same id should fail (already removed)
        let second = gate.respond("tu_dup", PermissionDecision::Deny);
        assert!(!second);

        let decision = rx.await.unwrap();
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn test_permission_decision_serialization() {
        let allow = PermissionDecision::Allow;
        let json = serde_json::to_value(&allow).unwrap();
        assert_eq!(json["type"], "allow");

        let deny = PermissionDecision::Deny;
        let json = serde_json::to_value(&deny).unwrap();
        assert_eq!(json["type"], "deny");

        let always = PermissionDecision::AllowAlways {
            rule: Some("Bash".to_string()),
        };
        let json = serde_json::to_value(&always).unwrap();
        assert_eq!(json["type"], "allow_always");
        assert_eq!(json["rule"], "Bash");
    }
}
