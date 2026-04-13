// Configuration file loading and management for BaoClaw
//
// Config file path: ~/.baoclaw/config.json

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// Default model name.
fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}

/// Default max retries per model.
fn default_max_retries() -> u32 {
    2
}

/// BaoClaw configuration loaded from ~/.baoclaw/config.json.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BaoclawConfig {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub fallback_models: Vec<String>,
    #[serde(default = "default_max_retries")]
    pub max_retries_per_model: u32,
    /// API type: "anthropic" (default) or "openai"
    #[serde(default = "default_api_type")]
    pub api_type: String,
    /// OpenAI-compatible API base URL (used when api_type is "openai")
    #[serde(default)]
    pub openai_base_url: Option<String>,
    /// Preserve unknown fields for forward compatibility.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn default_api_type() -> String { "anthropic".to_string() }

impl Default for BaoclawConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            fallback_models: Vec::new(),
            max_retries_per_model: default_max_retries(),
            api_type: default_api_type(),
            openai_base_url: None,
            extra: HashMap::new(),
        }
    }
}

/// Returns the config file path: ~/.baoclaw/config.json
pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".baoclaw").join("config.json")
}

/// Load configuration from ~/.baoclaw/config.json.
/// If the file does not exist, creates a default config file and returns defaults.
/// If the file contains invalid JSON, logs a warning and returns defaults.
/// Missing fields are filled with defaults; unknown fields are preserved.
pub fn load_config() -> BaoclawConfig {
    load_config_from(&config_path())
}

/// Load configuration from a specific path (for testing).
pub fn load_config_from(path: &std::path::Path) -> BaoclawConfig {
    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<BaoclawConfig>(&content) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Warning: invalid config JSON at {}: {}, using defaults", path.display(), e);
                BaoclawConfig::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Create default config file
            if let Err(write_err) = save_default_config_to(path) {
                eprintln!("Warning: could not create default config at {}: {}", path.display(), write_err);
            }
            BaoclawConfig::default()
        }
        Err(e) => {
            eprintln!("Warning: could not read config at {}: {}, using defaults", path.display(), e);
            BaoclawConfig::default()
        }
    }
}

/// Save the default configuration to ~/.baoclaw/config.json.
pub fn save_default_config() -> Result<(), std::io::Error> {
    save_default_config_to(&config_path())
}

/// Save the default configuration to a specific path (for testing).
pub fn save_default_config_to(path: &std::path::Path) -> Result<(), std::io::Error> {
    save_config_to(&BaoclawConfig::default(), path)
}

/// Save a configuration to a specific path.
pub fn save_config_to(config: &BaoclawConfig, path: &std::path::Path) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}

/// Apply environment variable overrides to the config.
/// If ANTHROPIC_MODEL is set, it overrides the primary model.
pub fn apply_env_override(config: &mut BaoclawConfig) {
    if let Ok(model) = std::env::var("ANTHROPIC_MODEL") {
        if !model.is_empty() {
            config.model = model;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn config_in(dir: &std::path::Path) -> PathBuf {
        dir.join("config.json")
    }

    #[test]
    fn test_default_values() {
        let config = BaoclawConfig::default();
        assert_eq!(config.model, "claude-sonnet-4-20250514");
        assert!(config.fallback_models.is_empty());
        assert_eq!(config.max_retries_per_model, 2);
        assert!(config.extra.is_empty());
    }

    #[test]
    fn test_file_not_exist_creates_default() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());
        assert!(!path.exists());

        let config = load_config_from(&path);
        assert_eq!(config.model, "claude-sonnet-4-20250514");
        assert!(path.exists(), "config file should be created");

        // Verify the created file is valid JSON with correct defaults
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: BaoclawConfig = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_invalid_json_fallback() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());
        std::fs::write(&path, "not valid json {{{").unwrap();

        let config = load_config_from(&path);
        assert_eq!(config.model, "claude-sonnet-4-20250514");
        assert_eq!(config.max_retries_per_model, 2);
    }

    #[test]
    fn test_missing_fields_filled() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());
        // Only specify model, missing fallback_models and max_retries_per_model
        std::fs::write(&path, r#"{"model": "claude-opus-4-20250514"}"#).unwrap();

        let config = load_config_from(&path);
        assert_eq!(config.model, "claude-opus-4-20250514");
        assert!(config.fallback_models.is_empty());
        assert_eq!(config.max_retries_per_model, 2);
    }

    #[test]
    fn test_unknown_fields_preserved() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());
        std::fs::write(&path, r#"{
            "model": "claude-sonnet-4-20250514",
            "fallback_models": [],
            "max_retries_per_model": 2,
            "future_feature": true,
            "theme": "dark"
        }"#).unwrap();

        let config = load_config_from(&path);
        assert_eq!(config.extra.get("future_feature"), Some(&Value::Bool(true)));
        assert_eq!(config.extra.get("theme"), Some(&Value::String("dark".to_string())));
    }

    #[test]
    fn test_env_override_model() {
        let mut config = BaoclawConfig::default();
        std::env::set_var("ANTHROPIC_MODEL", "claude-opus-4-20250514");
        apply_env_override(&mut config);
        assert_eq!(config.model, "claude-opus-4-20250514");
        std::env::remove_var("ANTHROPIC_MODEL");
    }

    #[test]
    fn test_env_override_not_set() {
        let mut config = BaoclawConfig::default();
        std::env::remove_var("ANTHROPIC_MODEL");
        apply_env_override(&mut config);
        assert_eq!(config.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());

        let original = BaoclawConfig {
            model: "claude-opus-4-20250514".to_string(),
            fallback_models: vec!["claude-sonnet-4-20250514".to_string(), "claude-3-5-haiku-20241022".to_string()],
            max_retries_per_model: 3,
            api_type: "anthropic".to_string(),
            openai_base_url: None,
            extra: {
                let mut m = HashMap::new();
                m.insert("custom_key".to_string(), Value::String("custom_value".to_string()));
                m
            },
        };

        save_config_to(&original, &path).unwrap();
        let loaded = load_config_from(&path);
        assert_eq!(original, loaded);
    }
}
