//! Data types for the multi-model routing system.
//!
//! This module defines the core types used for model selection,
//! routing rules, and routing decisions.

use serde::{Deserialize, Serialize};

/// Information about an available AI model.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    /// Short name for the model (e.g. "claude-sonnet-4").
    pub name: String,
    /// Provider name (e.g. "anthropic").
    pub provider: String,
    /// Maximum context window in tokens.
    pub max_tokens: u64,
    /// Cost per 1000 input tokens in USD.
    pub cost_per_1k_input: f64,
    /// Cost per 1000 output tokens in USD.
    pub cost_per_1k_output: f64,
    /// Capability tags (e.g. ["code", "reasoning", "vision"]).
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Priority when multiple models match (higher = more preferred).
    #[serde(default)]
    pub priority: i32,
}

impl ModelInfo {
    /// Create a new model info entry.
    pub fn new(
        name: &str,
        provider: &str,
        max_tokens: u64,
        cost_per_1k_input: f64,
        cost_per_1k_output: f64,
    ) -> Self {
        Self {
            name: name.to_string(),
            provider: provider.to_string(),
            max_tokens,
            cost_per_1k_input,
            cost_per_1k_output,
            capabilities: Vec::new(),
            priority: 0,
        }
    }

    /// Add capabilities to this model. Builder-style.
    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }

    /// Set priority. Builder-style.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Check if model has a specific capability.
    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }

    /// Estimate cost for a given number of input and output tokens.
    pub fn estimate_cost(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        let input_cost = (input_tokens as f64 / 1000.0) * self.cost_per_1k_input;
        let output_cost = (output_tokens as f64 / 1000.0) * self.cost_per_1k_output;
        input_cost + output_cost
    }
}

/// A condition that determines when a routing rule should match.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "params")]
pub enum RouteCondition {
    /// Match based on task complexity score (0.0 to 1.0).
    #[serde(rename = "task_complexity")]
    TaskComplexity {
        /// Minimum complexity (inclusive).
        min: f64,
        /// Maximum complexity (inclusive).
        max: f64,
    },
    /// Match based on message/prompt length in characters.
    #[serde(rename = "message_length")]
    MessageLength {
        /// Minimum message length in characters.
        min: usize,
        /// Maximum message length in characters.
        max: usize,
    },
    /// Match based on the number of files in context.
    #[serde(rename = "file_count")]
    FileCount {
        /// Minimum file count.
        min: usize,
        /// Maximum file count.
        max: usize,
    },
    /// Match based on time of day (for cost-saving during peak hours).
    #[serde(rename = "time_of_day")]
    TimeOfDay {
        /// Start hour (0-23 inclusive).
        hour_start: u32,
        /// End hour (0-23 inclusive).
        hour_end: u32,
    },
    /// Always matches — fallback/default rule.
    #[serde(rename = "always")]
    Always,
}

impl RouteCondition {
    /// Evaluate whether this condition matches the given context.
    pub fn matches(
        &self,
        prompt: &str,
        file_count: usize,
        complexity: f64,
        current_hour: u32,
    ) -> bool {
        match self {
            Self::TaskComplexity { min, max } => complexity >= *min && complexity <= *max,
            Self::MessageLength { min, max } => {
                let len = prompt.chars().count();
                len >= *min && len <= *max
            }
            Self::FileCount { min, max } => file_count >= *min && file_count <= *max,
            Self::TimeOfDay {
                hour_start,
                hour_end,
            } => {
                if hour_start <= hour_end {
                    current_hour >= *hour_start && current_hour <= *hour_end
                } else {
                    // Wraps around midnight, e.g. 22-06
                    current_hour >= *hour_start || current_hour <= *hour_end
                }
            }
            Self::Always => true,
        }
    }
}

/// A routing rule that maps a condition to a target model.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RoutingRule {
    /// Unique identifier for this rule.
    pub id: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// The condition that triggers this rule.
    pub condition: RouteCondition,
    /// The model to route to when the condition matches.
    pub target_model: String,
    /// Priority: higher-priority rules are checked first.
    #[serde(default)]
    pub priority: i32,
    /// Whether this rule is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// The result of a routing decision.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RoutingDecision {
    /// The selected model name.
    pub selected_model: String,
    /// Human-readable explanation for the decision.
    pub reason: String,
    /// Confidence level (0.0 to 1.0).
    pub confidence: f64,
}

impl RoutingDecision {
    /// Create a new routing decision.
    pub fn new(selected_model: &str, reason: &str, confidence: f64) -> Self {
        Self {
            selected_model: selected_model.to_string(),
            reason: reason.to_string(),
            confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_info_estimate_cost() {
        let model = ModelInfo::new("test-model", "test", 200_000, 0.003, 0.015);
        let cost = model.estimate_cost(5000, 2000);
        // input: 5000/1000 * 0.003 = 0.015
        // output: 2000/1000 * 0.015 = 0.030
        // total = 0.045
        assert!((cost - 0.045).abs() < 0.0001);
    }

    #[test]
    fn test_model_info_has_capability() {
        let model = ModelInfo::new("test", "test", 100_000, 0.001, 0.005)
            .with_capabilities(vec!["code".into(), "vision".into()]);
        assert!(model.has_capability("code"));
        assert!(model.has_capability("vision"));
        assert!(!model.has_capability("audio"));
    }

    #[test]
    fn test_model_info_builder() {
        let model = ModelInfo::new("test", "test", 100_000, 0.001, 0.005)
            .with_capabilities(vec!["code".into()])
            .with_priority(10);
        assert_eq!(model.priority, 10);
        assert_eq!(model.capabilities.len(), 1);
    }

    #[test]
    fn test_route_condition_task_complexity() {
        let cond = RouteCondition::TaskComplexity {
            min: 0.5,
            max: 0.8,
        };
        assert!(cond.matches("any prompt", 0, 0.6, 12));
        assert!(!cond.matches("any prompt", 0, 0.3, 12));
        assert!(!cond.matches("any prompt", 0, 0.9, 12));
    }

    #[test]
    fn test_route_condition_message_length() {
        let cond = RouteCondition::MessageLength {
            min: 10,
            max: 100,
        };
        assert!(cond.matches("hello world yes", 0, 0.0, 12));
        assert!(!cond.matches("hi", 0, 0.0, 12));
        assert!(!cond.matches(&"x".repeat(200), 0, 0.0, 12));
    }

    #[test]
    fn test_route_condition_file_count() {
        let cond = RouteCondition::FileCount { min: 3, max: 10 };
        assert!(cond.matches("", 5, 0.0, 12));
        assert!(!cond.matches("", 1, 0.0, 12));
        assert!(!cond.matches("", 15, 0.0, 12));
    }

    #[test]
    fn test_route_condition_time_of_day() {
        let cond = RouteCondition::TimeOfDay {
            hour_start: 9,
            hour_end: 17,
        };
        assert!(cond.matches("", 0, 0.0, 12));
        assert!(!cond.matches("", 0, 0.0, 8));
        assert!(!cond.matches("", 0, 0.0, 18));
    }

    #[test]
    fn test_route_condition_time_of_day_wraparound() {
        let cond = RouteCondition::TimeOfDay {
            hour_start: 22,
            hour_end: 6,
        };
        assert!(cond.matches("", 0, 0.0, 23));
        assert!(cond.matches("", 0, 0.0, 2));
        assert!(!cond.matches("", 0, 0.0, 12));
    }

    #[test]
    fn test_route_condition_always() {
        let cond = RouteCondition::Always;
        assert!(cond.matches("", 0, 0.0, 0));
        assert!(cond.matches("anything", 999, 0.5, 23));
    }

    #[test]
    fn test_routing_decision_new() {
        let d = RoutingDecision::new("claude-sonnet-4", "best fit", 0.9);
        assert_eq!(d.selected_model, "claude-sonnet-4");
        assert_eq!(d.reason, "best fit");
        assert_eq!(d.confidence, 0.9);
    }

    #[test]
    fn test_serialize_model_info() {
        let model = ModelInfo::new("claude-sonnet-4", "anthropic", 200_000, 3.0, 15.0)
            .with_capabilities(vec!["code".into(), "reasoning".into()])
            .with_priority(10);
        let json = serde_json::to_string(&model).unwrap();
        let parsed: ModelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(model, parsed);
    }

    #[test]
    fn test_serialize_route_condition() {
        let cond = RouteCondition::TaskComplexity {
            min: 0.3,
            max: 0.7,
        };
        let json = serde_json::to_string(&cond).unwrap();
        let parsed: RouteCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(cond, parsed);
    }

    #[test]
    fn test_serialize_routing_rule() {
        let rule = RoutingRule {
            id: "test-rule".into(),
            description: "Test".into(),
            condition: RouteCondition::Always,
            target_model: "claude-haiku".into(),
            priority: 5,
            enabled: true,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: RoutingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, parsed);
    }
}
