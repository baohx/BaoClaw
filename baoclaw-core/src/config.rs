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

/// Default model context window size in tokens (Claude default).
fn default_context_window() -> u64 {
    200_000
}

/// Default ratio of context window at which to auto-compact.
fn default_compact_threshold() -> f64 {
    0.7
}

// ─── ModelProfile ───────────────────────────────────────────────────────────

/// A model configuration profile with its own API credentials and window.
/// Each profile can independently specify api_type, api_key, base_url, etc.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelProfile {
    /// Model name (e.g., "glm-5.2", "deepseek-chat").
    pub model: String,
    /// API protocol: "anthropic" or "openai".
    pub api_type: String,
    /// API key (plaintext; will migrate to keychain later).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Base URL (e.g., "https://open.bigmodel.cn/api/anthropic").
    #[serde(default)]
    pub base_url: Option<String>,
    /// Context window in tokens.
    #[serde(default = "default_context_window")]
    pub context_window: u64,
    /// Auto-compact threshold ratio (0.0-1.0).
    #[serde(default = "default_compact_threshold")]
    pub auto_compact_threshold_ratio: f64,
    /// Max retries before falling back to next profile.
    #[serde(default = "default_max_retries")]
    pub max_retries_per_model: u32,
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
    /// Model context window size in tokens (e.g. 200_000 for Claude, 128_000 for GPT-4).
    #[serde(default = "default_context_window")]
    pub context_window: u64,
    /// Auto-compact threshold as fraction of context_window (e.g. 0.7 = 70%).
    #[serde(default = "default_compact_threshold")]
    pub auto_compact_threshold_ratio: f64,
    /// Maximum chars before tool output is persisted to disk (default 200_000).
    #[serde(default = "default_tool_output_threshold_chars")]
    pub tool_output_threshold_chars: usize,

    // === New: Named model profiles (P1-1) ===
    /// Named model profiles (new format). Each profile has its own api_type,
    /// api_key, base_url, context_window, etc.
    #[serde(default)]
    pub model_profiles: HashMap<String, ModelProfile>,
    /// Primary profile name (if using profiles format).
    #[serde(default)]
    pub primary_profile: Option<String>,
    /// Fallback profile names (if using profiles format).
    #[serde(default)]
    pub fallback_profiles: Vec<String>,

    /// Preserve unknown fields for forward compatibility.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn default_api_type() -> String { "anthropic".to_string() }
pub fn default_tool_output_threshold_chars() -> usize { 200_000 }

impl Default for BaoclawConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            fallback_models: Vec::new(),
            max_retries_per_model: default_max_retries(),
            api_type: default_api_type(),
            openai_base_url: None,
            context_window: default_context_window(),
            auto_compact_threshold_ratio: default_compact_threshold(),
            tool_output_threshold_chars: default_tool_output_threshold_chars(),
            model_profiles: HashMap::new(),
            primary_profile: None,
            fallback_profiles: Vec::new(),
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
    let mut config = match std::fs::read_to_string(path) {
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
    };

    // Normalize: auto-migrate old format to profiles if needed
    normalize_profiles(&mut config);
      // Sync new format back to old format for backward compatibility
      sync_profiles_to_legacy(&mut config);
    config
}

/// Normalize config: if old format (model + fallback_models strings),
/// auto-convert to new format (model_profiles + primary_profile).
/// This is called automatically by `load_config_from`.
pub fn normalize_profiles(config: &mut BaoclawConfig) {
    if !config.model_profiles.is_empty() {
        return; // Already using new format
    }
    if config.model.is_empty() {
        return; // Nothing to migrate
    }

    // Auto-migrate old format → profiles
    let primary = ModelProfile {
        model: config.model.clone(),
        api_type: config.api_type.clone(),
        api_key: None, // API keys come from env vars or profile.api_key
        base_url: config.openai_base_url.clone(),
        context_window: config.context_window,
        auto_compact_threshold_ratio: config.auto_compact_threshold_ratio,
        max_retries_per_model: config.max_retries_per_model,
    };
    config.model_profiles.insert("primary".to_string(), primary);
    config.primary_profile = Some("primary".to_string());

    for (i, m) in config.fallback_models.iter().enumerate() {
        let name = format!("fallback_{}", i);
        let p = ModelProfile {
            model: m.clone(),
            api_type: config.api_type.clone(), // inherit primary api_type
            api_key: None,
            base_url: config.openai_base_url.clone(),
            context_window: config.context_window,
            auto_compact_threshold_ratio: config.auto_compact_threshold_ratio,
            max_retries_per_model: config.max_retries_per_model,
        };
        config.model_profiles.insert(name.clone(), p);
        config.fallback_profiles.push(name);
    }
}

/// Sync new format back to old format fields for backward compatibility.
/// This ensures that code using config.model and config.fallback_models
/// still works when the config uses model_profiles.
pub fn sync_profiles_to_legacy(config: &mut BaoclawConfig) {
    if config.model_profiles.is_empty() {
        return; // Nothing to sync
    }
    
    // Sync primary model
    if let Some(ref primary_name) = config.primary_profile {
        if let Some(primary_profile) = config.model_profiles.get(primary_name.as_str()) {
            config.model = primary_profile.model.clone();
            config.api_type = primary_profile.api_type.clone();
            config.context_window = primary_profile.context_window;
            config.auto_compact_threshold_ratio = primary_profile.auto_compact_threshold_ratio;
        }
    }
    
    // Sync fallback_models from fallback_profiles
    config.fallback_models.clear();
    for fallback_name in config.fallback_profiles.iter() {
        if let Some(fallback_profile) = config.model_profiles.get(fallback_name.as_str()) {
            config.fallback_models.push(fallback_profile.model.clone());
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
        std::env::set_var("ANTHROPIC_MODEL", "claude-opus-4-20250514");
        let mut config = BaoclawConfig::default();
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

        let mut original = BaoclawConfig {
            model: "claude-opus-4-20250514".to_string(),
            fallback_models: vec!["claude-sonnet-4-20250514".to_string(), "claude-3-5-haiku-20241022".to_string()],
            max_retries_per_model: 3,
            api_type: "anthropic".to_string(),
            openai_base_url: None,
            context_window: 200_000,
            auto_compact_threshold_ratio: 0.7,
            tool_output_threshold_chars: 200_000,
            model_profiles: HashMap::new(),
            primary_profile: None,
            fallback_profiles: Vec::new(),
            extra: {
                let mut m = HashMap::new();
                m.insert("custom_key".to_string(), Value::String("custom_value".to_string()));
                m
            },
        };

        save_config_to(&original, &path).unwrap();
        let loaded = load_config_from(&path);
        // normalize_profiles runs during load_config_from, so normalize original too
        normalize_profiles(&mut original);
        assert_eq!(original, loaded);
    }

    #[test]
    fn test_auto_migrate_old_format() {
        let dir = TempDir::new().unwrap();
        let path = config_in(dir.path());

        // Old-format config: only model + fallback_models (no model_profiles)
        std::fs::write(&path, r#"{
            "model": "claude-sonnet-4-20250514",
            "fallback_models": ["claude-3-5-haiku-20241022", "claude-opus-4-20250514"],
            "api_type": "anthropic",
            "context_window": 200000
        }"#).unwrap();

        let config = load_config_from(&path);

        // primary_profile should be set
        assert_eq!(config.primary_profile.as_deref(), Some("primary"));

        // model_profiles should contain "primary" + 2 fallbacks
        assert_eq!(config.model_profiles.len(), 3);
        assert!(config.model_profiles.contains_key("primary"));
        assert!(config.model_profiles.contains_key("fallback_0"));
        assert!(config.model_profiles.contains_key("fallback_1"));

        // fallback_profiles should list the fallback names
        assert_eq!(config.fallback_profiles, vec!["fallback_0", "fallback_1"]);

        // Primary profile should carry the model name
        let primary = config.model_profiles.get("primary").unwrap();
        assert_eq!(primary.model, "claude-sonnet-4-20250514");
        assert_eq!(primary.api_type, "anthropic");
        assert_eq!(primary.context_window, 200_000);

        // Fallback profiles should inherit api_type from primary
        let fb0 = config.model_profiles.get("fallback_0").unwrap();
        assert_eq!(fb0.model, "claude-3-5-haiku-20241022");
        assert_eq!(fb0.api_type, "anthropic");

        let fb1 = config.model_profiles.get("fallback_1").unwrap();
        assert_eq!(fb1.model, "claude-opus-4-20250514");
        assert_eq!(fb1.api_type, "anthropic");
    }
}
