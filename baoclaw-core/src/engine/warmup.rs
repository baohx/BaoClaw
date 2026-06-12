//! Smart context warmup — predicts needed resources from user input and preloads them.
//!
//! Flow: user input ──► intent prediction ──► rule matching ──► warmup execution
//!   - warm the file cache (read files matching globs)
//!   - preload skills (interface reserved)
//!   - preload MCP tools (interface reserved)
//!
//! Learning: each warmed file that is later actually read counts as a "hit".
//! Rules with persistently low hit rates get their weight reduced and are
//! eventually skipped. Stats persist to `~/.baoclaw/warmup_stats.json`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::file_cache::FileCache;
use crate::engine::intent_predictor::PredictedIntent;

/// Default minimum samples before a rule's hit rate is considered meaningful.
const DEFAULT_MIN_SAMPLES: u32 = 10;
/// Default hit-rate threshold below which a rule's weight decays.
const DEFAULT_HIT_RATE_THRESHOLD: f64 = 0.6;
/// Weight below which a rule is no longer triggered.
const MIN_ACTIVE_WEIGHT: f64 = 0.2;
/// Max files warmed per rule (avoid IO storms).
const MAX_FILES_PER_RULE: usize = 20;

// ─────────────────────────────── Config (4.3) ───────────────────────────────

/// A single warmup rule: when `pattern` (or an intent) matches the user
/// message, the listed resources are preloaded.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WarmupRule {
    /// Unique rule identifier.
    #[serde(default)]
    pub id: String,
    /// Regex pattern matched against the user message (case-insensitive).
    pub pattern: String,
    /// Intent keys this rule applies to (e.g. "testing", "debug").
    /// Empty = pattern-only matching.
    #[serde(default)]
    pub intents: Vec<String>,
    /// MCP tools to preload (interface reserved).
    #[serde(default)]
    pub preload_mcp: Vec<String>,
    /// File globs to warm into the file cache.
    #[serde(default)]
    pub warmup_files: Vec<String>,
    /// Skills to preload (interface reserved).
    #[serde(default)]
    pub preload_skills: Vec<String>,
    /// Built-in tools to hint as likely-needed.
    #[serde(default)]
    pub warmup_tools: Vec<String>,
}

/// Learning parameters for warmup effectiveness tracking.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WarmupLearning {
    #[serde(default = "default_learning_enabled")]
    pub enabled: bool,
    /// Minimum samples before hit-rate is trusted.
    #[serde(default = "default_min_samples")]
    pub min_samples: u32,
    /// Hit rate below which a rule's weight decays.
    #[serde(default = "default_hit_rate_threshold")]
    pub hit_rate_threshold: f64,
}

fn default_learning_enabled() -> bool {
    true
}
fn default_min_samples() -> u32 {
    DEFAULT_MIN_SAMPLES
}
fn default_hit_rate_threshold() -> f64 {
    DEFAULT_HIT_RATE_THRESHOLD
}

impl Default for WarmupLearning {
    fn default() -> Self {
        Self {
            enabled: true,
            min_samples: DEFAULT_MIN_SAMPLES,
            hit_rate_threshold: DEFAULT_HIT_RATE_THRESHOLD,
        }
    }
}

/// Top-level warmup configuration (`~/.baoclaw/warmup.json`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WarmupConfig {
    #[serde(default)]
    pub rules: Vec<WarmupRule>,
    #[serde(default)]
    pub learning: WarmupLearning,
}

impl Default for WarmupConfig {
    fn default() -> Self {
        Self {
            rules: default_rules(),
            learning: WarmupLearning::default(),
        }
    }
}

/// Built-in default rules used when `warmup.json` does not exist.
fn default_rules() -> Vec<WarmupRule> {
    vec![
        WarmupRule {
            id: "testing".into(),
            pattern: r"test|测试".into(),
            intents: vec!["testing".into()],
            preload_mcp: vec![],
            warmup_files: vec!["tests/**/*.rs".into(), "**/*test*.ts".into()],
            preload_skills: vec!["test-driven-development".into()],
            warmup_tools: vec!["Bash".into()],
        },
        WarmupRule {
            id: "debugging".into(),
            pattern: r"error|bug|fix|crash|panic|报错|崩溃".into(),
            intents: vec!["debugging".into()],
            preload_mcp: vec![],
            warmup_files: vec!["**/*.log".into()],
            preload_skills: vec!["systematic-debugging".into()],
            warmup_tools: vec!["Bash".into(), "FileRead".into()],
        },
        WarmupRule {
            id: "deploy".into(),
            pattern: r"deploy|docker|kubernetes|k8s|部署".into(),
            intents: vec!["deployment".into()],
            preload_mcp: vec!["kubernetes".into(), "docker".into()],
            warmup_files: vec!["Dockerfile".into(), "deploy/**".into(), "k8s/**".into()],
            preload_skills: vec![],
            warmup_tools: vec!["Bash".into()],
        },
        WarmupRule {
            id: "refactor".into(),
            pattern: r"refactor|重构".into(),
            intents: vec!["refactoring".into()],
            preload_mcp: vec![],
            warmup_files: vec![],
            preload_skills: vec!["refactoring-patterns".into()],
            warmup_tools: vec!["Grep".into(), "FileRead".into(), "FileEdit".into()],
        },
        WarmupRule {
            id: "docs".into(),
            pattern: r"readme|document|docs|文档".into(),
            intents: vec!["doc_write".into()],
            preload_mcp: vec![],
            warmup_files: vec!["README.md".into(), "docs/**/*.md".into()],
            preload_skills: vec![],
            warmup_tools: vec!["FileWrite".into()],
        },
    ]
}

/// Returns the warmup config path: `~/.baoclaw/warmup.json`.
pub fn warmup_config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".baoclaw").join("warmup.json")
}

/// Returns the warmup stats path: `~/.baoclaw/warmup_stats.json`.
pub fn warmup_stats_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".baoclaw").join("warmup_stats.json")
}

impl WarmupConfig {
    /// Load from `~/.baoclaw/warmup.json`; returns defaults if missing/invalid.
    pub fn load() -> Self {
        Self::load_from(&warmup_config_path())
    }

    /// Load from an explicit path (for testing). Missing file → defaults.
    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<WarmupConfig>(&content) {
                Ok(mut cfg) => {
                    // Assign ids to rules missing one (use pattern as fallback).
                    for (i, rule) in cfg.rules.iter_mut().enumerate() {
                        if rule.id.is_empty() {
                            rule.id = format!("rule_{}", i);
                        }
                    }
                    cfg
                }
                Err(e) => {
                    eprintln!("warmup.json parse error, using defaults: {}", e);
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }
}

// ─────────────────────────────── Stats (4.4) ───────────────────────────────

/// Per-rule effectiveness statistics.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RuleStats {
    /// Number of times this rule was triggered.
    pub samples: u32,
    /// Number of warmed files that were later actually read.
    pub hits: u32,
    /// Total files warmed by this rule.
    pub warmed: u32,
    /// Current weight (1.0 = full strength; below MIN_ACTIVE_WEIGHT = disabled).
    #[serde(default = "default_weight")]
    pub weight: f64,
}

fn default_weight() -> f64 {
    1.0
}

impl RuleStats {
    /// Hit rate of warmed files (0.0 if nothing warmed yet).
    pub fn hit_rate(&self) -> f64 {
        if self.warmed == 0 {
            return 0.0;
        }
        self.hits as f64 / self.warmed as f64
    }
}

/// Persistent warmup statistics (`~/.baoclaw/warmup_stats.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct WarmupStats {
    /// rule id → stats.
    pub rules: HashMap<String, RuleStats>,
}

impl WarmupStats {
    pub fn load() -> Self {
        Self::load_from(&warmup_stats_path())
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&warmup_stats_path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(path, json)
    }
}

// ─────────────────────────────── Manager (4.2) ───────────────────────────────

/// Result of a warmup pass.
#[derive(Clone, Debug, Default)]
pub struct WarmupResult {
    /// Rules that matched and fired.
    pub matched_rules: Vec<String>,
    /// Files warmed into the cache.
    pub warmed_files: Vec<PathBuf>,
    /// Skills suggested for preload (interface reserved).
    pub preload_skills: Vec<String>,
    /// MCP tools suggested for preload (interface reserved).
    pub preload_mcp: Vec<String>,
    /// Built-in tools hinted.
    pub warmup_tools: Vec<String>,
}

/// Manages context warmup: rule matching, file-cache warmup, learning.
pub struct WarmupManager {
    config: WarmupConfig,
    stats: WarmupStats,
    stats_path: PathBuf,
    /// Optional shared file cache to warm.
    file_cache: Option<Arc<tokio::sync::Mutex<FileCache>>>,
    /// Working directory for glob resolution.
    cwd: PathBuf,
    /// Files warmed in the current window, mapped to the rule(s) that warmed
    /// them. Used for hit attribution when files are later read.
    pending: HashMap<PathBuf, HashSet<String>>,
}

impl WarmupManager {
    /// Create a manager with config/stats loaded from default locations.
    pub fn new(cwd: PathBuf, file_cache: Option<Arc<tokio::sync::Mutex<FileCache>>>) -> Self {
        Self::with_config(WarmupConfig::load(), WarmupStats::load(), warmup_stats_path(), cwd, file_cache)
    }

    /// Create with explicit config/stats (for testing).
    pub fn with_config(
        config: WarmupConfig,
        stats: WarmupStats,
        stats_path: PathBuf,
        cwd: PathBuf,
        file_cache: Option<Arc<tokio::sync::Mutex<FileCache>>>,
    ) -> Self {
        Self {
            config,
            stats,
            stats_path,
            file_cache,
            cwd,
            pending: HashMap::new(),
        }
    }

    /// Match warmup rules against a user message and predicted intents.
    /// Rules whose weight has decayed below `MIN_ACTIVE_WEIGHT` are skipped.
    pub fn match_rules(&self, message: &str, intents: &[PredictedIntent]) -> Vec<&WarmupRule> {
        let intent_keys: HashSet<&str> = intents.iter().map(|p| p.intent.key()).collect();
        let mut matched = Vec::new();

        for rule in &self.config.rules {
            // Skip rules eliminated by learning
            if let Some(stats) = self.stats.rules.get(&rule.id) {
                if stats.weight < MIN_ACTIVE_WEIGHT {
                    continue;
                }
            }

            let intent_match = rule.intents.iter().any(|i| intent_keys.contains(i.as_str()));
            let pattern_match = regex::RegexBuilder::new(&rule.pattern)
                .case_insensitive(true)
                .build()
                .map(|re| re.is_match(message))
                .unwrap_or(false);

            if intent_match || pattern_match {
                matched.push(rule);
            }
        }
        matched
    }

    /// Run a full warmup pass for a user message. Never panics; IO errors are
    /// silently ignored (warmup is best-effort).
    pub async fn warmup(&mut self, message: &str, intents: &[PredictedIntent]) -> WarmupResult {
        let matched: Vec<WarmupRule> = self
            .match_rules(message, intents)
            .into_iter()
            .cloned()
            .collect();

        let mut result = WarmupResult::default();

        for rule in &matched {
            result.matched_rules.push(rule.id.clone());
            result.preload_skills.extend(rule.preload_skills.iter().cloned());
            result.preload_mcp.extend(rule.preload_mcp.iter().cloned());
            result.warmup_tools.extend(rule.warmup_tools.iter().cloned());

            // File cache warmup
            let files = self.resolve_globs(&rule.warmup_files);
            for path in files.into_iter().take(MAX_FILES_PER_RULE) {
                if self.warm_file(&path).await {
                    self.pending
                        .entry(path.clone())
                        .or_default()
                        .insert(rule.id.clone());
                    result.warmed_files.push(path);
                }
            }

            // Record sample + warmed count
            let stats = self.stats.rules.entry(rule.id.clone()).or_insert_with(|| RuleStats {
                weight: 1.0,
                ..Default::default()
            });
            stats.samples += 1;
        }

        // Count warmed files per rule
        for path in &result.warmed_files {
            if let Some(rule_ids) = self.pending.get(path) {
                for rid in rule_ids.clone() {
                    if let Some(s) = self.stats.rules.get_mut(&rid) {
                        s.warmed += 1;
                    }
                }
            }
        }

        // Preload skills/MCP: interface reserved — actual loading is performed
        // by the host (CLI) which owns the skill registry and MCP clients.

        let _ = self.stats.save_to(&self.stats_path);
        result
    }

    /// Resolve glob patterns relative to cwd into existing file paths.
    fn resolve_globs(&self, patterns: &[String]) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for pat in patterns {
            let full = self.cwd.join(pat);
            let pat_str = full.to_string_lossy().to_string();
            if let Ok(entries) = glob::glob(&pat_str) {
                for entry in entries.flatten() {
                    if entry.is_file() {
                        out.push(entry);
                    }
                }
            }
        }
        out
    }

    /// Warm a single file into the cache. Returns true on success.
    async fn warm_file(&self, path: &Path) -> bool {
        // Skip huge files (>1 MB) — warming them is counterproductive.
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > 1_048_576 {
                return false;
            }
        } else {
            return false;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return false,
        };

        if let Some(ref cache) = self.file_cache {
            let mut cache = cache.lock().await;
            cache.record(path, &content);
        }
        true
    }

    /// Record that a file was actually read (hit attribution, 4.4).
    /// If the file was previously warmed, the responsible rules get a hit.
    pub fn record_file_access(&mut self, path: &Path) {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(rule_ids) = self.pending.remove(&canonical) {
            for rid in rule_ids {
                if let Some(s) = self.stats.rules.get_mut(&rid) {
                    s.hits += 1;
                }
            }
            let _ = self.stats.save_to(&self.stats_path);
        }
    }

    /// Apply learning: adjust rule weights from hit rates (4.4).
    ///
    /// - hit rate ≥ threshold → weight recovers toward 1.0
    /// - hit rate < threshold (after min_samples) → weight decays by 20%
    /// - weight < MIN_ACTIVE_WEIGHT → rule no longer triggers
    pub fn apply_learning(&mut self) {
        if !self.config.learning.enabled {
            return;
        }
        let min_samples = self.config.learning.min_samples;
        let threshold = self.config.learning.hit_rate_threshold;

        for stats in self.stats.rules.values_mut() {
            if stats.samples < min_samples {
                continue;
            }
            if stats.hit_rate() < threshold {
                stats.weight *= 0.8;
            } else {
                stats.weight = (stats.weight + 0.1).min(1.0);
            }
        }
        let _ = self.stats.save_to(&self.stats_path);
    }

    /// Whether a rule is still active (weight above elimination threshold).
    pub fn is_rule_active(&self, rule_id: &str) -> bool {
        self.stats
            .rules
            .get(rule_id)
            .map(|s| s.weight >= MIN_ACTIVE_WEIGHT)
            .unwrap_or(true)
    }

    /// Read-only access to current stats.
    pub fn stats(&self) -> &WarmupStats {
        &self.stats
    }

    /// Read-only access to current config.
    pub fn config(&self) -> &WarmupConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::intent_predictor::UserIntent;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("baoclaw_warmup_test_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pred(intent: UserIntent, confidence: f64) -> PredictedIntent {
        PredictedIntent {
            intent,
            confidence,
            suggested_preloads: vec![],
        }
    }

    fn test_manager(dir: &Path) -> WarmupManager {
        WarmupManager::with_config(
            WarmupConfig::default(),
            WarmupStats::default(),
            dir.join("warmup_stats.json"),
            dir.to_path_buf(),
            None,
        )
    }

    #[test]
    fn test_config_default_has_rules() {
        let cfg = WarmupConfig::default();
        assert!(!cfg.rules.is_empty());
        assert!(cfg.learning.enabled);
        assert_eq!(cfg.learning.min_samples, DEFAULT_MIN_SAMPLES);
    }

    #[test]
    fn test_config_load_missing_file_returns_defaults() {
        let cfg = WarmupConfig::load_from(Path::new("/nonexistent/warmup.json"));
        assert_eq!(cfg, WarmupConfig::default());
    }

    #[test]
    fn test_config_load_from_json() {
        let dir = tmp_dir("cfg");
        let path = dir.join("warmup.json");
        std::fs::write(
            &path,
            r#"{
                "rules": [
                    {"pattern": "test|测试", "warmup_files": ["tests/**/*.py"], "preload_skills": ["testing-checklist"]}
                ],
                "learning": {"enabled": true, "min_samples": 5, "hit_rate_threshold": 0.5}
            }"#,
        )
        .unwrap();
        let cfg = WarmupConfig::load_from(&path);
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(cfg.rules[0].id, "rule_0"); // auto-assigned
        assert_eq!(cfg.rules[0].pattern, "test|测试");
        assert_eq!(cfg.learning.min_samples, 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_load_invalid_json_returns_defaults() {
        let dir = tmp_dir("badcfg");
        let path = dir.join("warmup.json");
        std::fs::write(&path, "{not valid json").unwrap();
        let cfg = WarmupConfig::load_from(&path);
        assert_eq!(cfg, WarmupConfig::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_match_rules_by_pattern() {
        let dir = tmp_dir("match_pat");
        let mgr = test_manager(&dir);
        let matched = mgr.match_rules("please fix this error", &[]);
        assert!(matched.iter().any(|r| r.id == "debugging"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_match_rules_by_intent() {
        let dir = tmp_dir("match_int");
        let mgr = test_manager(&dir);
        // Message doesn't contain pattern keywords, but intent matches
        let intents = vec![pred(UserIntent::Deployment, 0.8)];
        let matched = mgr.match_rules("ship it to production", &intents);
        assert!(matched.iter().any(|r| r.id == "deploy"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_match_rules_chinese_pattern() {
        let dir = tmp_dir("match_cn");
        let mgr = test_manager(&dir);
        let matched = mgr.match_rules("帮我写一个测试", &[]);
        assert!(matched.iter().any(|r| r.id == "testing"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_match_rules_no_match() {
        let dir = tmp_dir("match_none");
        let mgr = test_manager(&dir);
        let matched = mgr.match_rules("hello there", &[]);
        assert!(matched.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_warmup_warms_files_into_cache() {
        let dir = tmp_dir("warm_files");
        // Create a docs file matched by the "docs" rule
        std::fs::write(dir.join("README.md"), "# Test Project").unwrap();

        let cache = Arc::new(tokio::sync::Mutex::new(FileCache::new(10)));
        let mut mgr = WarmupManager::with_config(
            WarmupConfig::default(),
            WarmupStats::default(),
            dir.join("warmup_stats.json"),
            dir.clone(),
            Some(Arc::clone(&cache)),
        );

        let result = mgr.warmup("update the readme docs", &[]).await;
        assert!(result.matched_rules.contains(&"docs".to_string()));
        assert_eq!(result.warmed_files.len(), 1);
        assert_eq!(cache.lock().await.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_warmup_records_stats_and_persists() {
        let dir = tmp_dir("stats_persist");
        std::fs::write(dir.join("README.md"), "# Hi").unwrap();
        let stats_path = dir.join("warmup_stats.json");

        let mut mgr = WarmupManager::with_config(
            WarmupConfig::default(),
            WarmupStats::default(),
            stats_path.clone(),
            dir.clone(),
            None,
        );
        let _ = mgr.warmup("update docs please", &[]).await;

        assert!(stats_path.exists());
        let loaded = WarmupStats::load_from(&stats_path);
        let s = loaded.rules.get("docs").expect("docs rule stats recorded");
        assert_eq!(s.samples, 1);
        assert_eq!(s.warmed, 1);
        assert_eq!(s.hits, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_hit_attribution_on_file_access() {
        let dir = tmp_dir("hits");
        std::fs::write(dir.join("README.md"), "# Hi").unwrap();
        let mut mgr = test_manager(&dir);

        let result = mgr.warmup("write some docs", &[]).await;
        assert_eq!(result.warmed_files.len(), 1);

        // Simulate the agent actually reading the warmed file
        mgr.record_file_access(&result.warmed_files[0]);
        let s = mgr.stats().rules.get("docs").unwrap();
        assert_eq!(s.hits, 1);
        assert_eq!(s.hit_rate(), 1.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_learning_decays_low_hit_rate_rules() {
        let dir = tmp_dir("learn_decay");
        let mut mgr = test_manager(&dir);
        // Simulate a rule with many samples but poor hit rate
        mgr.stats.rules.insert(
            "debugging".into(),
            RuleStats {
                samples: 20,
                hits: 1,
                warmed: 20,
                weight: 1.0,
            },
        );
        mgr.apply_learning();
        let w = mgr.stats.rules.get("debugging").unwrap().weight;
        assert!(w < 1.0, "weight should decay, got {}", w);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_learning_recovers_good_rules() {
        let dir = tmp_dir("learn_recover");
        let mut mgr = test_manager(&dir);
        mgr.stats.rules.insert(
            "docs".into(),
            RuleStats {
                samples: 20,
                hits: 18,
                warmed: 20,
                weight: 0.5,
            },
        );
        mgr.apply_learning();
        let w = mgr.stats.rules.get("docs").unwrap().weight;
        assert!(w > 0.5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_learning_skips_under_sampled_rules() {
        let dir = tmp_dir("learn_skip");
        let mut mgr = test_manager(&dir);
        mgr.stats.rules.insert(
            "testing".into(),
            RuleStats {
                samples: 2, // below min_samples (10)
                hits: 0,
                warmed: 2,
                weight: 1.0,
            },
        );
        mgr.apply_learning();
        assert_eq!(mgr.stats.rules.get("testing").unwrap().weight, 1.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_low_weight_rule_not_triggered() {
        let dir = tmp_dir("eliminated");
        let mut mgr = test_manager(&dir);
        mgr.stats.rules.insert(
            "debugging".into(),
            RuleStats {
                samples: 50,
                hits: 0,
                warmed: 50,
                weight: 0.1, // below MIN_ACTIVE_WEIGHT
            },
        );
        assert!(!mgr.is_rule_active("debugging"));
        let matched = mgr.match_rules("fix this error", &[]);
        assert!(!matched.iter().any(|r| r.id == "debugging"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_stats_load_missing_returns_default() {
        let stats = WarmupStats::load_from(Path::new("/nonexistent/stats.json"));
        assert!(stats.rules.is_empty());
    }

    #[test]
    fn test_stats_roundtrip() {
        let dir = tmp_dir("stats_rt");
        let path = dir.join("warmup_stats.json");
        let mut stats = WarmupStats::default();
        stats.rules.insert(
            "x".into(),
            RuleStats {
                samples: 3,
                hits: 2,
                warmed: 3,
                weight: 0.9,
            },
        );
        stats.save_to(&path).unwrap();
        let loaded = WarmupStats::load_from(&path);
        assert_eq!(loaded, stats);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_warmup_collects_skills_and_mcp() {
        let dir = tmp_dir("skills");
        let mut mgr = test_manager(&dir);
        let result = mgr.warmup("deploy with docker", &[]).await;
        assert!(result.matched_rules.contains(&"deploy".to_string()));
        assert!(result.preload_mcp.contains(&"docker".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
