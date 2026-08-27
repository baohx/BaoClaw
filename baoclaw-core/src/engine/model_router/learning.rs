//! Router learning — records routing decisions and learns from outcomes.
//!
//! Uses SQLite to store routing decision history, then analyzes the data
//! to suggest optimal model assignments and auto-adjust routing rules.

use super::types::RoutingRule;
use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Metrics collected from the routing system.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RouterMetrics {
    /// Total number of routing decisions made.
    pub total_routes: u64,
    /// Number of routes where user was satisfied with the result.
    pub successful_routes: u64,
    /// Overall success rate (0.0 to 1.0).
    pub success_rate: f64,
    /// Average cost savings compared to always using the most expensive model, as a percentage.
    pub avg_cost_savings_pct: f64,
    /// Total cost incurred across all routes.
    pub total_cost: f64,
    /// Number of routes that used each model.
    pub model_usage: Vec<(String, u64)>,
    /// Average tokens used per route.
    pub avg_tokens_per_route: u64,
}

/// Tracks routing decisions and learns optimal model assignments.
///
/// Stores data in SQLite at `~/.baoclaw/router_stats.db` and provides
/// methods to record outcomes, query for optimal models, and auto-generate
/// routing rules based on historical data.
pub struct RouterLearning {
    conn: Connection,
}

impl RouterLearning {
    /// Open (or create) the router stats database.
    ///
    /// Creates the `route_decisions` table if it doesn't exist.
    pub fn new() -> Result<Self, String> {
        let path = stats_db_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create stats directory: {}", e))?;
        }

        let conn = Connection::open(&path)
            .map_err(|e| format!("Failed to open router stats DB: {}", e))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS route_decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                prompt_preview TEXT NOT NULL,
                prompt_complexity REAL NOT NULL,
                file_count INTEGER NOT NULL DEFAULT 0,
                model_used TEXT NOT NULL,
                tokens_used INTEGER NOT NULL DEFAULT 0,
                cost REAL NOT NULL DEFAULT 0.0,
                user_satisfied INTEGER NOT NULL DEFAULT 0,
                confidence REAL NOT NULL DEFAULT 0.0
            )",
            [],
        )
        .map_err(|e| format!("Failed to create table: {}", e))?;

        // Create index for common queries
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_route_decisions_complexity
             ON route_decisions(prompt_complexity)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_route_decisions_model
             ON route_decisions(model_used)",
            [],
        );

        Ok(Self { conn })
    }

    /// Record a routing decision outcome.
    ///
    /// # Arguments
    ///
    /// * `prompt_complexity` - The complexity score (0.0 to 1.0).
    /// * `model_used` - The model that was selected.
    /// * `tokens_used` - Total tokens consumed.
    /// * `cost` - Actual cost in USD.
    /// * `user_satisfied` - Whether the user was satisfied with the result.
    pub fn record_decision(
        &self,
        prompt_complexity: f64,
        model_used: &str,
        tokens_used: u64,
        cost: f64,
        user_satisfied: bool,
    ) -> Result<(), String> {
        let satisfied_int: i32 = if user_satisfied { 1 } else { 0 };

        self.conn
            .execute(
                "INSERT INTO route_decisions
                 (prompt_preview, prompt_complexity, file_count, model_used, tokens_used, cost, user_satisfied, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    "",
                    prompt_complexity,
                    0_i64,
                    model_used,
                    tokens_used as i64,
                    cost,
                    satisfied_int,
                    0.5_f64,
                ],
            )
            .map_err(|e| format!("Failed to record decision: {}", e))?;

        Ok(())
    }

    /// Find the optimal model for a given task complexity based on historical data.
    ///
    /// Looks at past decisions with similar complexity and selects the model
    /// with the highest satisfaction rate.
    ///
    /// Returns `None` if there isn't enough data.
    pub fn optimal_model_for_complexity(&self, complexity: f64) -> Option<String> {
        // Find decisions within ±0.15 complexity range
        let min = (complexity - 0.15).max(0.0);
        let max = (complexity + 0.15).min(1.0);

        let result: SqlResult<Option<String>> = self.conn.query_row(
            "SELECT model_used, COUNT(*) as cnt, AVG(user_satisfied) as satisfaction
             FROM route_decisions
             WHERE prompt_complexity BETWEEN ?1 AND ?2
             GROUP BY model_used
             HAVING cnt >= 3
             ORDER BY satisfaction DESC, cnt DESC
             LIMIT 1",
            params![min, max],
            |row| row.get(0),
        );

        match result {
            Ok(Some(model)) => Some(model),
            Ok(None) | Err(_) => None,
        }
    }

    /// Automatically generate routing rules from historical data.
    ///
    /// Analyzes the route decision history and creates rules that
    /// map complexity ranges to the best-performing models.
    ///
    /// Each generated rule maps a specific complexity band to the
    /// model with the highest satisfaction rate in that band.
    pub fn adjust_rules(&self) -> Vec<RoutingRule> {
        let bands = vec![(0.0, 0.25), (0.25, 0.5), (0.5, 0.75), (0.75, 1.0)];

        let mut rules = Vec::new();

        for (min, max) in bands {
            let model_result: SqlResult<Option<String>> = self.conn.query_row(
                "SELECT model_used, COUNT(*) as cnt, AVG(user_satisfied) as satisfaction
                 FROM route_decisions
                 WHERE prompt_complexity >= ?1 AND prompt_complexity < ?2
                 GROUP BY model_used
                 HAVING cnt >= 3
                 ORDER BY satisfaction DESC, cnt DESC
                 LIMIT 1",
                params![min, max],
                |row| row.get(0),
            );

            if let Ok(Some(model)) = model_result {
                rules.push(RoutingRule {
                    id: format!("auto-complexity-{:.0}-{:.0}", min * 100.0, max * 100.0),
                    description: format!(
                        "Auto-generated: complexity {:.2}-{:.2} → {}",
                        min, max, model
                    ),
                    condition: super::types::RouteCondition::TaskComplexity { min, max },
                    target_model: model,
                    priority: 50, // Medium priority for auto-generated rules
                    enabled: true,
                });
            }
        }

        rules
    }

    /// Collect and return routing metrics.
    pub fn get_metrics(&self) -> RouterMetrics {
        let mut metrics = RouterMetrics::default();

        // Total routes
        if let Ok(count) = self
            .conn
            .query_row("SELECT COUNT(*) FROM route_decisions", [], |row| {
                row.get::<_, i64>(0)
            })
        {
            metrics.total_routes = count as u64;
        }

        // Successful routes
        if let Ok(count) = self.conn.query_row(
            "SELECT COUNT(*) FROM route_decisions WHERE user_satisfied = 1",
            [],
            |row| row.get::<_, i64>(0),
        ) {
            metrics.successful_routes = count as u64;
        }

        // Success rate
        if metrics.total_routes > 0 {
            metrics.success_rate = metrics.successful_routes as f64 / metrics.total_routes as f64;
        }

        // Total cost
        if let Ok(cost) = self.conn.query_row(
            "SELECT COALESCE(SUM(cost), 0) FROM route_decisions",
            [],
            |row| row.get::<_, f64>(0),
        ) {
            metrics.total_cost = cost;
        }

        // Model usage
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT model_used, COUNT(*) as cnt
             FROM route_decisions
             GROUP BY model_used
             ORDER BY cnt DESC",
        ) {
            if let Ok(rows) = stmt.query_map([], |row| {
                let model: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((model, count as u64))
            }) {
                for row in rows.flatten() {
                    metrics.model_usage.push(row);
                }
            }
        }

        // Average tokens per route
        if let Ok(avg_tokens) = self.conn.query_row(
            "SELECT COALESCE(AVG(tokens_used), 0) FROM route_decisions",
            [],
            |row| row.get::<_, f64>(0),
        ) {
            metrics.avg_tokens_per_route = avg_tokens as u64;
        }

        // Average cost savings: compare actual cost to what it would have been
        // with the most expensive model. Simplified calculation.
        if metrics.total_routes > 0 {
            // Find the max cost among models
            if let Ok(max_cost_per_token) = self.conn.query_row(
                "SELECT MAX(cost / NULLIF(tokens_used, 0)) FROM route_decisions",
                [],
                |row| row.get::<_, f64>(0),
            ) {
                if max_cost_per_token > 0.0 {
                    let total_tokens: u64 = metrics
                        .avg_tokens_per_route
                        .saturating_mul(metrics.total_routes);
                    let hypothetical_max_cost = total_tokens as f64 * max_cost_per_token;
                    if hypothetical_max_cost > 0.0 {
                        metrics.avg_cost_savings_pct =
                            ((hypothetical_max_cost - metrics.total_cost) / hypothetical_max_cost)
                                * 100.0;
                    }
                }
            }
        }

        metrics
    }

    /// Get access to the underlying SQLite connection (for testing).
    #[doc(hidden)]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Create a RouterLearning with an in-memory database (for testing).
    #[doc(hidden)]
    pub fn new_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Failed to open in-memory DB: {}", e))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS route_decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                prompt_preview TEXT NOT NULL,
                prompt_complexity REAL NOT NULL,
                file_count INTEGER NOT NULL DEFAULT 0,
                model_used TEXT NOT NULL,
                tokens_used INTEGER NOT NULL DEFAULT 0,
                cost REAL NOT NULL DEFAULT 0.0,
                user_satisfied INTEGER NOT NULL DEFAULT 0,
                confidence REAL NOT NULL DEFAULT 0.0
            )",
            [],
        )
        .map_err(|e| format!("Failed to create table: {}", e))?;

        Ok(Self { conn })
    }
}

/// Get the default stats DB path: `~/.baoclaw/router_stats.db`.
fn stats_db_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".baoclaw").join("router_stats.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_in_memory() {
        let learning = RouterLearning::new_in_memory();
        assert!(learning.is_ok());
    }

    #[test]
    fn test_record_decision() {
        let learning = RouterLearning::new_in_memory().unwrap();
        let result = learning.record_decision(0.3, "claude-3-5-haiku-20241022", 5000, 0.02, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_optimal_model_for_complexity_not_enough_data() {
        let learning = RouterLearning::new_in_memory().unwrap();
        // Only 2 records - not enough for a recommendation (needs >= 3)
        learning
            .record_decision(0.2, "claude-3-5-haiku-20241022", 1000, 0.005, true)
            .unwrap();
        learning
            .record_decision(0.2, "claude-3-5-haiku-20241022", 1200, 0.006, true)
            .unwrap();
        let optimal = learning.optimal_model_for_complexity(0.2);
        assert!(optimal.is_none());
    }

    #[test]
    fn test_optimal_model_for_complexity_enough_data() {
        let learning = RouterLearning::new_in_memory().unwrap();

        // Record multiple decisions in the same complexity range
        for _ in 0..5 {
            learning
                .record_decision(0.25, "claude-3-5-haiku-20241022", 1000, 0.005, true)
                .unwrap();
        }
        // Also record some with a different model
        learning
            .record_decision(0.25, "claude-sonnet-4-20250514", 1000, 0.015, true)
            .unwrap();

        let optimal = learning.optimal_model_for_complexity(0.25);
        // Haiku has more records and higher satisfaction
        assert!(optimal.is_some());
        assert_eq!(optimal.unwrap(), "claude-3-5-haiku-20241022");
    }

    #[test]
    fn test_adjust_rules_generates_rules() {
        let learning = RouterLearning::new_in_memory().unwrap();

        // Populate data across complexity bands
        for _ in 0..5 {
            learning
                .record_decision(0.1, "claude-3-5-haiku-20241022", 500, 0.002, true)
                .unwrap();
        }
        for _ in 0..5 {
            learning
                .record_decision(0.6, "claude-sonnet-4-20250514", 2000, 0.03, true)
                .unwrap();
        }
        for _ in 0..5 {
            learning
                .record_decision(0.85, "claude-opus-4-20250514", 5000, 0.15, true)
                .unwrap();
        }

        let rules = learning.adjust_rules();
        // Should have at least some rules generated
        assert!(!rules.is_empty());

        // Each rule should have the auto- prefix
        for rule in &rules {
            assert!(rule.id.starts_with("auto-complexity-"));
            assert_eq!(rule.priority, 50);
            assert!(rule.enabled);
        }
    }

    #[test]
    fn test_get_metrics_empty() {
        let learning = RouterLearning::new_in_memory().unwrap();
        let metrics = learning.get_metrics();
        assert_eq!(metrics.total_routes, 0);
        assert_eq!(metrics.successful_routes, 0);
        assert_eq!(metrics.success_rate, 0.0);
        assert_eq!(metrics.total_cost, 0.0);
    }

    #[test]
    fn test_get_metrics_with_data() {
        let learning = RouterLearning::new_in_memory().unwrap();

        learning
            .record_decision(0.3, "claude-3-5-haiku-20241022", 1000, 0.005, true)
            .unwrap();
        learning
            .record_decision(0.7, "claude-sonnet-4-20250514", 3000, 0.045, true)
            .unwrap();
        learning
            .record_decision(0.9, "claude-opus-4-20250514", 8000, 0.50, false)
            .unwrap();

        let metrics = learning.get_metrics();
        assert_eq!(metrics.total_routes, 3);
        assert_eq!(metrics.successful_routes, 2);
        assert!((metrics.success_rate - 2.0 / 3.0).abs() < 0.01);
        assert!((metrics.total_cost - 0.55).abs() < 0.01);

        // All 3 models should appear
        assert_eq!(metrics.model_usage.len(), 3);
        // Average tokens
        assert_eq!(metrics.avg_tokens_per_route, 4000); // (1000+3000+8000)/3
    }

    #[test]
    fn test_record_decision_multiple() {
        let learning = RouterLearning::new_in_memory().unwrap();

        for i in 0..10 {
            let satisfied = i % 2 == 0;
            learning
                .record_decision(0.5, "claude-sonnet-4-20250514", 2000, 0.03, satisfied)
                .unwrap();
        }

        let metrics = learning.get_metrics();
        assert_eq!(metrics.total_routes, 10);
        assert_eq!(metrics.successful_routes, 5);
    }
}
