//! Sandbox configuration file loading and management.
//!
//! Config file path: ~/.baoclaw/sandbox.json

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::profile::SandboxProfile;

/// Sandbox configuration file structure.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SandboxConfigFile {
    /// Named sandbox profiles.
    #[serde(default)]
    pub profiles: HashMap<String, SandboxProfile>,

    /// Automatic profile selection rules.
    #[serde(default)]
    pub auto_profile: HashMap<String, String>,

    /// Default profile name to use.
    #[serde(default = "default_profile")]
    pub default_profile: String,

    /// Whether to ask for confirmation on profile upgrade.
    #[serde(default = "default_ask_on_upgrade")]
    pub ask_on_upgrade: bool,
}

fn default_profile() -> String {
    "read_only".to_string()
}

fn default_ask_on_upgrade() -> bool {
    true
}

impl Default for SandboxConfigFile {
    fn default() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert("read_only".to_string(), SandboxProfile::read_only());
        profiles.insert("web_dev".to_string(), SandboxProfile::web_dev());
        profiles.insert("full_access".to_string(), SandboxProfile::full_access());

        let mut auto_profile = HashMap::new();
        auto_profile.insert("FileRead".to_string(), "read_only".to_string());
        auto_profile.insert("FileWrite".to_string(), "ask".to_string());
        auto_profile.insert("Bash:npm install".to_string(), "web_dev".to_string());
        auto_profile.insert("Bash:git".to_string(), "full_access".to_string());

        Self {
            profiles,
            auto_profile,
            default_profile: default_profile(),
            ask_on_upgrade: true,
        }
    }
}

impl SandboxConfigFile {
    /// Get a profile by name.
    pub fn get_profile(&self, name: &str) -> Option<&SandboxProfile> {
        self.profiles.get(name)
    }

    /// Determine the appropriate profile for a tool operation.
    /// Returns the profile name or "ask" if confirmation is needed.
    pub fn resolve_profile(&self, tool_name: &str, command: Option<&str>) -> Option<String> {
        // Try exact tool:command match first
        if let Some(cmd) = command {
            let key = format!("{}:{}", tool_name, cmd);
            if let Some(profile) = self.auto_profile.get(&key) {
                return Some(profile.clone());
            }
        }

        // Try tool-only match
        if let Some(profile) = self.auto_profile.get(tool_name) {
            return Some(profile.clone());
        }

        // Return default profile
        Some(self.default_profile.clone())
    }

    /// Add or update a profile.
    pub fn set_profile(&mut self, name: String, profile: SandboxProfile) {
        self.profiles.insert(name, profile);
    }

    /// Set an auto-profile rule.
    pub fn set_auto_profile(&mut self, trigger: String, profile: String) {
        self.auto_profile.insert(trigger, profile);
    }
}

/// Returns the sandbox config file path: ~/.baoclaw/sandbox.json
pub fn sandbox_config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".baoclaw").join("sandbox.json")
}

/// Load sandbox configuration from ~/.baoclaw/sandbox.json.
/// If the file does not exist, creates a default config file and returns defaults.
pub fn load_sandbox_config() -> SandboxConfigFile {
    load_sandbox_config_from(&sandbox_config_path())
}

/// Load sandbox configuration from a specific path (for testing).
pub fn load_sandbox_config_from(path: &std::path::Path) -> SandboxConfigFile {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            match serde_json::from_str::<SandboxConfigFile>(&content) {
                Ok(config) => {
                    // Merge with defaults to ensure all preset profiles exist
                    let mut defaults = SandboxConfigFile::default();
                    for (name, profile) in config.profiles {
                        defaults.profiles.insert(name, profile);
                    }
                    defaults.auto_profile = config.auto_profile;
                    defaults.default_profile = config.default_profile;
                    defaults.ask_on_upgrade = config.ask_on_upgrade;
                    defaults
                }
                Err(e) => {
                    eprintln!(
                        "Warning: invalid sandbox config JSON at {}: {}, using defaults",
                        path.display(),
                        e
                    );
                    SandboxConfigFile::default()
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Create default config file
            if let Err(write_err) = save_sandbox_config_to(&SandboxConfigFile::default(), path) {
                eprintln!(
                    "Warning: could not create default sandbox config at {}: {}",
                    path.display(),
                    write_err
                );
            }
            SandboxConfigFile::default()
        }
        Err(e) => {
            eprintln!(
                "Warning: could not read sandbox config at {}: {}, using defaults",
                path.display(),
                e
            );
            SandboxConfigFile::default()
        }
    }
}

/// Save sandbox configuration to a specific path.
pub fn save_sandbox_config_to(
    config: &SandboxConfigFile,
    path: &std::path::Path,
) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn config_in(dir: &std::path::Path) -> PathBuf {
        dir.join("sandbox.json")
    }

    #[test]
    fn test_default_config() {
        let config = SandboxConfigFile::default();
        assert!(config.profiles.contains_key("read_only"));
        assert!(config.profiles.contains_key("web_dev"));
        assert!(config.profiles.contains_key("full_access"));
        assert_eq!(config.default_profile, "read_only");
        assert!(config.ask_on_upgrade);
    }

    #[test]
    fn test_get_profile() {
        let config = SandboxConfigFile::default();
        let profile = config.get_profile("read_only").unwrap();
        assert!(!profile.network.is_allowed());
    }

    #[test]
    fn test_resolve_profile() {
        let config = SandboxConfigFile::default();

        // FileRead should use read_only
        assert_eq!(
            config.resolve_profile("FileRead", None),
            Some("read_only".to_string())
        );

        // npm install should use web_dev
        assert_eq!(
            config.resolve_profile("Bash", Some("npm install")),
            Some("web_dev".to_string())
        );

        // FileWrite should return "ask"
        assert_eq!(
            config.resolve_profile("FileWrite", None),
            Some("ask".to_string())
        );
    }

    #[test]
    fn test_file_not_exist_creates_default() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());
        assert!(!path.exists());

        let config = load_sandbox_config_from(&path);
        assert!(config.profiles.contains_key("read_only"));
        assert!(path.exists(), "config file should be created");

        // Verify the created file is valid JSON
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: SandboxConfigFile = serde_json::from_str(&content).unwrap();
        assert!(parsed.profiles.contains_key("web_dev"));
    }

    #[test]
    fn test_invalid_json_fallback() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());
        std::fs::write(&path, "not valid json {{{").unwrap();

        let config = load_sandbox_config_from(&path);
        assert!(config.profiles.contains_key("read_only"));
    }

    #[test]
    fn test_merge_with_defaults() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());
        // Custom config with only a custom profile
        std::fs::write(
            &path,
            r#"{
            "profiles": {
                "custom": {
                    "name": "custom",
                    "writable_paths": ["/workspace"],
                    "network": true
                }
            },
            "default_profile": "custom"
        }"#,
        )
        .unwrap();

        let config = load_sandbox_config_from(&path);

        // Custom profile should exist
        assert!(config.profiles.contains_key("custom"));
        let custom = config.get_profile("custom").unwrap();
        assert!(custom.is_writable("/workspace/file"));

        // Preset profiles should still exist
        assert!(config.profiles.contains_key("read_only"));
        assert!(config.profiles.contains_key("web_dev"));

        // Default should be custom
        assert_eq!(config.default_profile, "custom");
    }

    #[test]
    fn test_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());

        let original = SandboxConfigFile::default();
        save_sandbox_config_to(&original, &path).unwrap();
        let loaded = load_sandbox_config_from(&path);

        assert_eq!(original.default_profile, loaded.default_profile);
        assert_eq!(original.ask_on_upgrade, loaded.ask_on_upgrade);
    }
}
