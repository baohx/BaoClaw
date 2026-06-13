//! Sandbox profile definitions.
//!
//! Profiles define security boundaries for sandboxed execution.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Network access rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NetworkRule {
    /// No network access.
    Disabled(bool),
    /// Full network access.
    Enabled(bool),
    /// Whitelist-based access with domain/port restrictions.
    Whitelist(Vec<String>),
}

impl Default for NetworkRule {
    fn default() -> Self {
        NetworkRule::Disabled(false)
    }
}

impl NetworkRule {
    /// Check if network is allowed (either full or whitelisted).
    pub fn is_allowed(&self) -> bool {
        match self {
            NetworkRule::Enabled(true) => true,
            NetworkRule::Whitelist(rules) if !rules.is_empty() => true,
            _ => false,
        }
    }

    /// Check if a specific host:port is allowed.
    pub fn is_host_allowed(&self, host: &str, port: Option<u16>) -> bool {
        match self {
            NetworkRule::Enabled(true) => true,
            NetworkRule::Disabled(false) | NetworkRule::Enabled(false) | NetworkRule::Disabled(true) => false,
            NetworkRule::Whitelist(rules) => {
                for rule in rules {
                    if Self::matches_rule(rule, host, port) {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Match a host:port against a rule pattern.
    /// Patterns: "localhost:*", "*.npmjs.org", "github.com:443", "*:443"
    fn matches_rule(rule: &str, host: &str, port: Option<u16>) -> bool {
        let parts: Vec<&str> = rule.rsplitn(2, ':').collect();
        let (host_pattern, port_pattern) = if parts.len() == 2 {
            (parts[1], Some(parts[0]))
        } else {
            // No port specified means any port
            (rule, None)
        };

        // Check port first
        if let Some(pp) = port_pattern {
            if pp != "*" {
                let allowed_port: u16 = match pp.parse() {
                    Ok(p) => p,
                    Err(_) => return false,
                };
                if port != Some(allowed_port) {
                    return false;
                }
            }
        }

        // Check host
        if host_pattern == "*" {
            return true;
        }
        if host_pattern.starts_with("*.") {
            let suffix = &host_pattern[1..]; // Remove leading "*"
            return host.ends_with(suffix);
        }
        host == host_pattern
    }
}

/// Predefined sandbox profiles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProfilePreset {
    #[serde(rename = "read_only")]
    ReadOnly,
    #[serde(rename = "web_dev")]
    WebDev,
    #[serde(rename = "full_access")]
    FullAccess,
    #[serde(rename = "custom")]
    Custom,
}

impl Default for ProfilePreset {
    fn default() -> Self {
        Self::ReadOnly
    }
}

/// Sandbox profile defining security boundaries.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SandboxProfile {
    /// Profile name/identifier.
    #[serde(default)]
    pub name: String,
    
    /// Paths that can be written to inside the sandbox.
    #[serde(default)]
    pub writable_paths: Vec<String>,
    
    /// Paths that can be read inside the sandbox.
    /// ["*"] means all paths readable.
    #[serde(default = "default_readable_paths")]
    pub readable_paths: Vec<String>,
    
    /// Network access rule.
    #[serde(default)]
    pub network: NetworkRule,
    
    /// Environment variables allowed to pass through.
    #[serde(default = "default_env_whitelist")]
    pub env_whitelist: Vec<String>,
    
    /// Memory limit in MB (0 = unlimited).
    #[serde(default)]
    pub max_memory_mb: u32,
    
    /// Execution timeout in seconds (0 = unlimited).
    #[serde(default)]
    pub timeout_secs: u32,
    
    /// CPU time limit in seconds (0 = unlimited).
    #[serde(default)]
    pub cpu_time_limit_secs: u32,
}

fn default_readable_paths() -> Vec<String> {
    vec!["*".to_string()]
}

fn default_env_whitelist() -> Vec<String> {
    vec!["PATH".to_string(), "HOME".to_string(), "USER".to_string()]
}

impl Default for SandboxProfile {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            writable_paths: Vec::new(),
            readable_paths: default_readable_paths(),
            network: NetworkRule::default(),
            env_whitelist: default_env_whitelist(),
            max_memory_mb: 0,
            timeout_secs: 0,
            cpu_time_limit_secs: 0,
        }
    }
}

impl SandboxProfile {
    /// Create a read-only profile with no network access.
    pub fn read_only() -> Self {
        Self {
            name: "read_only".to_string(),
            writable_paths: Vec::new(),
            readable_paths: vec!["*".to_string()],
            network: NetworkRule::Disabled(false),
            env_whitelist: vec!["PATH".to_string(), "HOME".to_string()],
            max_memory_mb: 0,
            timeout_secs: 0,
            cpu_time_limit_secs: 0,
        }
    }

    /// Create a web development profile with common web dev permissions.
    pub fn web_dev() -> Self {
        Self {
            name: "web_dev".to_string(),
            writable_paths: vec![
                "src/".to_string(),
                "dist/".to_string(),
                "node_modules/".to_string(),
                "package.json".to_string(),
            ],
            readable_paths: vec!["*".to_string()],
            network: NetworkRule::Whitelist(vec![
                "localhost:*".to_string(),
                "*.npmjs.org".to_string(),
                "*.github.com".to_string(),
            ]),
            env_whitelist: vec![
                "PATH".to_string(),
                "HOME".to_string(),
                "USER".to_string(),
                "NODE_PATH".to_string(),
            ],
            max_memory_mb: 1024,
            timeout_secs: 300,
            cpu_time_limit_secs: 0,
        }
    }

    /// Create a full access profile (minimal restrictions).
    pub fn full_access() -> Self {
        Self {
            name: "full_access".to_string(),
            writable_paths: vec!["*".to_string()],
            readable_paths: vec!["*".to_string()],
            network: NetworkRule::Enabled(true),
            env_whitelist: vec!["*".to_string()],
            max_memory_mb: 0,
            timeout_secs: 0,
            cpu_time_limit_secs: 0,
        }
    }

    /// Check if a path is writable.
    pub fn is_writable(&self, path: &str) -> bool {
        if self.writable_paths.contains(&"*".to_string()) {
            return true;
        }
        for allowed in &self.writable_paths {
            if path.starts_with(allowed) || Self::glob_match(allowed, path) {
                return true;
            }
        }
        false
    }

    /// Check if a path is readable.
    pub fn is_readable(&self, path: &str) -> bool {
        if self.readable_paths.contains(&"*".to_string()) {
            return true;
        }
        for allowed in &self.readable_paths {
            if path.starts_with(allowed) || Self::glob_match(allowed, path) {
                return true;
            }
        }
        false
    }

    /// Simple glob match supporting * wildcard.
    fn glob_match(pattern: &str, text: &str) -> bool {
        if !pattern.contains('*') {
            return pattern == text;
        }
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            let prefix = parts[0];
            let suffix = parts[1];
            text.starts_with(prefix) && text.ends_with(suffix)
        } else {
            // Complex glob - just use starts_with for now
            text.starts_with(pattern.trim_end_matches('*'))
        }
    }

    /// Filter environment variables according to whitelist.
    pub fn filter_env(&self, env: &[(String, String)]) -> Vec<(String, String)> {
        if self.env_whitelist.contains(&"*".to_string()) {
            return env.to_vec();
        }
        let whitelist: HashSet<&str> = self.env_whitelist.iter().map(|s| s.as_str()).collect();
        env.iter()
            .filter(|(key, _)| whitelist.contains(key.as_str()))
            .cloned()
            .collect()
    }

    /// Merge this profile with another (other takes precedence).
    pub fn merge(&self, other: &SandboxProfile) -> SandboxProfile {
        SandboxProfile {
            name: if other.name.is_empty() { self.name.clone() } else { other.name.clone() },
            writable_paths: if other.writable_paths.is_empty() { self.writable_paths.clone() } else { other.writable_paths.clone() },
            readable_paths: if other.readable_paths.is_empty() { self.readable_paths.clone() } else { other.readable_paths.clone() },
            network: if other.network == NetworkRule::default() { self.network.clone() } else { other.network.clone() },
            env_whitelist: if other.env_whitelist.is_empty() { self.env_whitelist.clone() } else { other.env_whitelist.clone() },
            max_memory_mb: if other.max_memory_mb == 0 { self.max_memory_mb } else { other.max_memory_mb },
            timeout_secs: if other.timeout_secs == 0 { self.timeout_secs } else { other.timeout_secs },
            cpu_time_limit_secs: if other.cpu_time_limit_secs == 0 { self.cpu_time_limit_secs } else { other.cpu_time_limit_secs },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_rule_disabled() {
        let rule = NetworkRule::Disabled(false);
        assert!(!rule.is_allowed());
        assert!(!rule.is_host_allowed("google.com", Some(443)));
    }

    #[test]
    fn test_network_rule_enabled() {
        let rule = NetworkRule::Enabled(true);
        assert!(rule.is_allowed());
        assert!(rule.is_host_allowed("google.com", Some(443)));
        assert!(rule.is_host_allowed("any.host", None));
    }

    #[test]
    fn test_network_rule_whitelist() {
        let rule = NetworkRule::Whitelist(vec![
            "localhost:*".to_string(),
            "*.npmjs.org".to_string(),
            "github.com:443".to_string(),
        ]);
        assert!(rule.is_allowed());
        
        // localhost any port
        assert!(rule.is_host_allowed("localhost", Some(3000)));
        assert!(rule.is_host_allowed("localhost", Some(8080)));
        
        // npmjs subdomain
        assert!(rule.is_host_allowed("registry.npmjs.org", Some(443)));
        assert!(rule.is_host_allowed("api.npmjs.org", Some(443)));
        
        // github specific port
        assert!(rule.is_host_allowed("github.com", Some(443)));
        assert!(!rule.is_host_allowed("github.com", Some(80)));
        
        // not whitelisted
        assert!(!rule.is_host_allowed("google.com", Some(443)));
    }

    #[test]
    fn test_profile_read_only() {
        let profile = SandboxProfile::read_only();
        assert!(!profile.is_writable("/tmp/file"));
        assert!(profile.is_readable("/tmp/file"));
        assert!(!profile.network.is_allowed());
        assert!(profile.env_whitelist.contains(&"PATH".to_string()));
    }

    #[test]
    fn test_profile_web_dev() {
        let profile = SandboxProfile::web_dev();
        assert!(profile.is_writable("src/main.rs"));
        assert!(profile.is_writable("dist/bundle.js"));
        assert!(!profile.is_writable("/etc/passwd"));
        assert!(profile.network.is_host_allowed("registry.npmjs.org", Some(443)));
        assert_eq!(profile.max_memory_mb, 1024);
        assert_eq!(profile.timeout_secs, 300);
    }

    #[test]
    fn test_profile_full_access() {
        let profile = SandboxProfile::full_access();
        assert!(profile.is_writable("/any/path"));
        assert!(profile.is_readable("/any/path"));
        assert!(profile.network.is_allowed());
        assert!(profile.env_whitelist.contains(&"*".to_string()));
    }

    #[test]
    fn test_filter_env() {
        let profile = SandboxProfile::read_only();
        let env = vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
            ("SECRET_KEY".to_string(), "secret123".to_string()),
        ];
        let filtered = profile.filter_env(&env);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|(k, _)| k == "PATH"));
        assert!(filtered.iter().any(|(k, _)| k == "HOME"));
        assert!(!filtered.iter().any(|(k, _)| k == "SECRET_KEY"));
    }

    #[test]
    fn test_filter_env_wildcard() {
        let profile = SandboxProfile::full_access();
        let env = vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("SECRET_KEY".to_string(), "secret123".to_string()),
        ];
        let filtered = profile.filter_env(&env);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_merge_profiles() {
        let base = SandboxProfile::read_only();
        let override_profile = SandboxProfile {
            name: "custom".to_string(),
            writable_paths: vec!["/workspace".to_string()],
            ..Default::default()
        };
        let merged = base.merge(&override_profile);
        assert_eq!(merged.name, "custom");
        assert!(merged.is_writable("/workspace/file"));
        assert!(!merged.network.is_allowed()); // inherited from base
    }

    #[test]
    fn test_profile_serialization() {
        let profile = SandboxProfile::web_dev();
        let json = serde_json::to_string(&profile).unwrap();
        let parsed: SandboxProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(profile, parsed);
    }
}
