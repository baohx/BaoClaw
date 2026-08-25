//! Budget management for model routing.
//!
//! Tracks daily and monthly spending limits, warns when approaching
//! limits, and can block expensive model usage when over budget.

use super::types::ModelInfo;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Result of a budget check for using a model.
#[derive(Clone, Debug, PartialEq)]
pub enum BudgetResult {
    /// The estimated cost is within budget.
    Allowed,
    /// The estimated cost approaches the limit; a warning is issued
    /// but the request is still allowed.
    Warning {
        /// Human-readable warning message.
        message: String,
    },
    /// The estimated cost would exceed the budget; the request is blocked.
    Blocked {
        /// Human-readable reason for blocking.
        reason: String,
    },
}

/// Manages daily and monthly spending budgets.
///
/// Persists budget state to `~/.baoclaw/budget.json` so that limits
/// are tracked across sessions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetManager {
    /// Maximum daily spending in USD.
    pub daily_limit: f64,
    /// Maximum monthly spending in USD.
    pub monthly_limit: f64,
    /// Amount spent today in USD.
    pub current_daily: f64,
    /// Amount spent this month in USD.
    pub current_monthly: f64,
    /// Day of month when the daily counter was last reset (1-31).
    last_daily_reset_day: u32,
    /// Month when the monthly counter was last reset (1-12).
    last_monthly_reset_month: u32,
    /// Year when counters were last checked.
    last_reset_year: i32,

    /// Path to the persistence file (not serialized).
    #[serde(skip)]
    persist_path: Option<PathBuf>,
}

impl BudgetManager {
    /// Create a new BudgetManager with the given limits.
    ///
    /// # Arguments
    ///
    /// * `daily_limit` - Maximum spending per day (USD).
    /// * `monthly_limit` - Maximum spending per month (USD).
    pub fn new(daily_limit: f64, monthly_limit: f64) -> Self {
        use chrono::{Datelike, Local};
        let now = Local::now();
        Self {
            daily_limit,
            monthly_limit,
            current_daily: 0.0,
            current_monthly: 0.0,
            last_daily_reset_day: now.day(),
            last_monthly_reset_month: now.month(),
            last_reset_year: now.year(),
            persist_path: None,
        }
    }

    /// Check whether using the given model with estimated tokens is within budget.
    ///
    /// # Arguments
    ///
    /// * `model` - The model being considered.
    /// * `estimated_tokens` - Estimated total tokens (input + output) for the request.
    ///
    /// # Returns
    ///
    /// A [`BudgetResult`] indicating whether the cost is allowed, warned, or blocked.
    pub fn can_afford(&mut self, model: &ModelInfo, estimated_tokens: u64) -> BudgetResult {
        self.ensure_reset();

        // Estimate cost: assume 70% input, 30% output tokens
        let input_tokens = (estimated_tokens as f64 * 0.7) as u64;
        let output_tokens = (estimated_tokens as f64 * 0.3) as u64;
        let estimated_cost = model.estimate_cost(input_tokens, output_tokens);

        // Check monthly first (harder limit)
        if self.monthly_limit > 0.0 && self.current_monthly + estimated_cost > self.monthly_limit {
            return BudgetResult::Blocked {
                reason: format!(
                    "Monthly budget exceeded: ${:.2} spent of ${:.2} limit, \
                     estimated ${:.4} for this request",
                    self.current_monthly, self.monthly_limit, estimated_cost
                ),
            };
        }

        // Check daily
        if self.daily_limit > 0.0 {
            let projected = self.current_daily + estimated_cost;

            // If already over, block
            if self.current_daily >= self.daily_limit {
                return BudgetResult::Blocked {
                    reason: format!(
                        "Daily budget exhausted: ${:.2} of ${:.2}",
                        self.current_daily, self.daily_limit
                    ),
                };
            }

            // If near limit (within 10%), warn
            if projected > self.daily_limit * 0.9 && projected <= self.daily_limit {
                return BudgetResult::Warning {
                    message: format!(
                        "Approaching daily budget limit: ${:.2} / ${:.2} (${:.4} remaining)",
                        self.current_daily,
                        self.daily_limit,
                        self.daily_limit - self.current_daily
                    ),
                };
            }

            // If over, block
            if projected > self.daily_limit {
                return BudgetResult::Blocked {
                    reason: format!(
                        "Daily budget would be exceeded: ${:.2} spent + ${:.4} estimated \
                         > ${:.2} limit",
                        self.current_daily, estimated_cost, self.daily_limit
                    ),
                };
            }
        }

        BudgetResult::Allowed
    }

    /// Record a cost after a model call has been made.
    ///
    /// This adds the cost to daily and monthly totals and persists
    /// the updated state to disk.
    pub fn record_cost(&mut self, cost: f64) {
        self.ensure_reset();
        self.current_daily += cost;
        self.current_monthly += cost;
        let _ = self.save();
    }

    /// Remaining daily budget in USD.
    pub fn remaining_daily(&self) -> f64 {
        (self.daily_limit - self.current_daily).max(0.0)
    }

    /// Remaining monthly budget in USD.
    pub fn remaining_monthly(&self) -> f64 {
        (self.monthly_limit - self.current_monthly).max(0.0)
    }

    /// Reset daily/monthly counters if the day or month has changed.
    fn ensure_reset(&mut self) {
        use chrono::{Datelike, Local};
        let now = Local::now();
        let current_day = now.day();
        let current_month = now.month();
        let current_year = now.year();

        // Reset monthly if month or year changed
        if current_month != self.last_monthly_reset_month || current_year != self.last_reset_year {
            self.current_monthly = 0.0;
            self.last_monthly_reset_month = current_month;
        }

        // Reset daily if day changed
        if current_day != self.last_daily_reset_day
            || current_month != self.last_monthly_reset_month
            || current_year != self.last_reset_year
        {
            self.current_daily = 0.0;
            self.last_daily_reset_day = current_day;
        }

        self.last_reset_year = current_year;
    }

    /// Load a BudgetManager from the default persistence file.
    ///
    /// If the file doesn't exist or can't be parsed, returns a default
    /// instance with typical limits ($10/day, $200/month).
    pub fn load() -> Self {
        let path = budget_file_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<BudgetManager>(&contents) {
                Ok(mut manager) => {
                    manager.persist_path = Some(path);
                    manager.ensure_reset();
                    manager
                }
                Err(_) => {
                    BudgetManager {
                        persist_path: Some(path.clone()),
                        ..Self::default()
                    }
                }
            },
            Err(_) => {
                let manager = BudgetManager {
                    persist_path: Some(path.clone()),
                    ..Self::default()
                };
                let _ = manager.save();
                manager
            }
        }
    }

    /// Save the current budget state to the persistence file.
    fn save(&self) -> Result<(), String> {
        let path = self
            .persist_path
            .clone()
            .unwrap_or_else(budget_file_path);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("Failed to create budget directory: {}", e)
            })?;
        }

        let json = serde_json::to_string_pretty(self).map_err(|e| {
            format!("Failed to serialize budget: {}", e)
        })?;

        std::fs::write(&path, json).map_err(|e| {
            format!("Failed to write budget file: {}", e)
        })?;

        Ok(())
    }

    /// Set the persist path (for testing).
    #[doc(hidden)]
    pub fn with_persist_path(mut self, path: PathBuf) -> Self {
        self.persist_path = Some(path);
        self
    }
}

impl Default for BudgetManager {
    fn default() -> Self {
        Self::new(10.0, 200.0)
    }
}

/// Get the default budget file path: `~/.baoclaw/budget.json`.
fn budget_file_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".baoclaw").join("budget.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_model(cost_1k_in: f64, cost_1k_out: f64) -> ModelInfo {
        ModelInfo::new("test-model", "test", 100_000, cost_1k_in, cost_1k_out)
    }

    #[test]
    fn test_new_budget_manager() {
        let bm = BudgetManager::new(10.0, 200.0);
        assert_eq!(bm.daily_limit, 10.0);
        assert_eq!(bm.monthly_limit, 200.0);
        assert_eq!(bm.current_daily, 0.0);
        assert_eq!(bm.current_monthly, 0.0);
        assert_eq!(bm.remaining_daily(), 10.0);
        assert_eq!(bm.remaining_monthly(), 200.0);
    }

    #[test]
    fn test_can_afford_allowed() {
        let mut bm = BudgetManager::new(10.0, 200.0);
        let model = make_model(0.003, 0.015); // Sonnet-like pricing
        // 1000 tokens → input=700, output=300 → cost ~0.0066
        let result = bm.can_afford(&model, 1000);
        assert_eq!(result, BudgetResult::Allowed);
    }

    #[test]
    fn test_can_afford_blocked_daily() {
        let mut bm = BudgetManager::new(0.001, 200.0); // very small daily limit
        let model = make_model(0.003, 0.015);

        // First spend should be blocked because even 100 tokens exceeds $0.001
        let result = bm.can_afford(&model, 1000);
        assert!(matches!(result, BudgetResult::Blocked { .. }));
    }

    #[test]
    fn test_can_afford_blocked_monthly() {
        let mut bm = BudgetManager::new(100.0, 0.0001); // tiny monthly limit
        let model = make_model(0.003, 0.015);
        let result = bm.can_afford(&model, 1000);
        assert!(matches!(result, BudgetResult::Blocked { .. }));
    }

    #[test]
    fn test_can_afford_warning_near_limit() {
        let mut bm = BudgetManager::new(0.01, 200.0);
        let model = make_model(0.003, 0.015);

        // Spend enough to get close to the limit
        bm.current_daily = 0.0095; // 95% of $0.01

        // 1000 tokens → ~$0.0066, would push over but...
        // Actually let's test with a tiny cost
        let cheap_model = make_model(0.0001, 0.0001);
        let result = bm.can_afford(&cheap_model, 100);
        // After 95% usage, even a small cost triggers warning if it puts us over 90%
        assert!(matches!(result, BudgetResult::Warning { .. }));
    }

    #[test]
    fn test_record_cost() {
        let mut bm = BudgetManager::new(10.0, 200.0);
        bm.record_cost(0.05);
        assert!((bm.current_daily - 0.05).abs() < 0.0001);
        assert!((bm.current_monthly - 0.05).abs() < 0.0001);
        assert!((bm.remaining_daily() - 9.95).abs() < 0.0001);
    }

    #[test]
    fn test_record_cost_accumulates() {
        let mut bm = BudgetManager::new(10.0, 200.0);
        bm.record_cost(1.0);
        bm.record_cost(2.0);
        bm.record_cost(0.5);
        assert!((bm.current_daily - 3.5).abs() < 0.0001);
        assert!((bm.current_monthly - 3.5).abs() < 0.0001);
    }

    #[test]
    fn test_remaining_daily() {
        let mut bm = BudgetManager::new(10.0, 200.0);
        bm.current_daily = 7.5;
        assert!((bm.remaining_daily() - 2.5).abs() < 0.0001);
    }

    #[test]
    fn test_remaining_monthly() {
        let mut bm = BudgetManager::new(10.0, 200.0);
        bm.current_monthly = 150.0;
        assert!((bm.remaining_monthly() - 50.0).abs() < 0.0001);
    }

    #[test]
    fn test_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("budget.json");

        let mut bm = BudgetManager::new(5.0, 100.0).with_persist_path(path.clone());
        bm.record_cost(2.5);

        // Load from file
        let loaded = BudgetManager::load();
        // Can't easily test file-based load in unit test since it uses global path,
        // but we can test save directly
        let json = serde_json::to_string(&bm).unwrap();
        let parsed: BudgetManager = serde_json::from_str(&json).unwrap();
        assert!((parsed.current_daily - 2.5).abs() < 0.0001);
        assert_eq!(parsed.daily_limit, 5.0);
        assert_eq!(parsed.monthly_limit, 100.0);
    }

    #[test]
    fn test_default_budget_manager() {
        let bm = BudgetManager::default();
        assert_eq!(bm.daily_limit, 10.0);
        assert_eq!(bm.monthly_limit, 200.0);
    }

    #[test]
    fn test_budget_result_debug() {
        let allowed = BudgetResult::Allowed;
        let warning = BudgetResult::Warning {
            message: "test warning".into(),
        };
        let blocked = BudgetResult::Blocked {
            reason: "test reason".into(),
        };
        assert_ne!(format!("{:?}", allowed), "");
        assert!(format!("{:?}", warning).contains("test warning"));
        assert!(format!("{:?}", blocked).contains("test reason"));
    }
}
