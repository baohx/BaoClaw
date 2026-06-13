//! Model Router — the core routing engine.
//!
//! The `ModelRouter` maintains a list of available models and routing rules,
//! and selects the best model for a given task based on the rules.

use super::types::{ModelInfo, RoutingDecision, RoutingRule};

/// The model routing engine.
///
/// Maintains a list of available AI models and a set of routing rules.
/// When `route()` is called, it evaluates rules in priority order and
/// returns the best matching model.
#[derive(Clone, Debug)]
pub struct ModelRouter {
    models: Vec<ModelInfo>,
    rules: Vec<RoutingRule>,
}

impl ModelRouter {
    /// Create a new ModelRouter with sensible defaults.
    ///
    /// Pre-loads three Claude models with actual Anthropic pricing
    /// (prices in USD per 1K tokens):
    ///
    /// | Model              | Input/1K  | Output/1K | Priority |
    /// |--------------------|-----------|-----------|----------|
    /// | claude-sonnet-4    | $0.003    | $0.015    | 10       |
    /// | claude-opus-4      | $0.015    | $0.075    | 5        |
    /// | claude-3-5-haiku   | $0.0008   | $0.004    | 1        |
    pub fn new() -> Self {
        let models = vec![
            ModelInfo::new(
                "claude-sonnet-4-20250514",
                "anthropic",
                200_000,
                0.003,  // $3.00 / 1M tokens → $0.003 / 1K
                0.015,  // $15.00 / 1M tokens → $0.015 / 1K
            )
            .with_capabilities(vec![
                "code".into(),
                "reasoning".into(),
                "vision".into(),
                "tool_use".into(),
            ])
            .with_priority(10),
            ModelInfo::new(
                "claude-opus-4-20250514",
                "anthropic",
                200_000,
                0.015, // $15.00 / 1M tokens → $0.015 / 1K
                0.075, // $75.00 / 1M tokens → $0.075 / 1K
            )
            .with_capabilities(vec![
                "code".into(),
                "reasoning".into(),
                "vision".into(),
                "tool_use".into(),
                "advanced_reasoning".into(),
            ])
            .with_priority(5),
            ModelInfo::new(
                "claude-3-5-haiku-20241022",
                "anthropic",
                200_000,
                0.0008, // $0.80 / 1M tokens → $0.0008 / 1K
                0.004,  // $4.00 / 1M tokens → $0.004 / 1K
            )
            .with_capabilities(vec![
                "code".into(),
                "tool_use".into(),
                "fast".into(),
            ])
            .with_priority(1),
        ];

        Self {
            models,
            rules: Vec::new(),
        }
    }

    /// Add a model to the router.
    ///
    /// If a model with the same name already exists, it is replaced.
    pub fn add_model(&mut self, model: ModelInfo) {
        if let Some(existing) = self.models.iter_mut().find(|m| m.name == model.name) {
            *existing = model;
        } else {
            self.models.push(model);
        }
    }

    /// Remove a model by name. Returns true if a model was removed.
    pub fn remove_model(&mut self, name: &str) -> bool {
        let len_before = self.models.len();
        self.models.retain(|m| m.name != name);
        self.models.len() < len_before
    }

    /// Add a routing rule.
    ///
    /// If a rule with the same ID already exists, it is replaced.
    pub fn add_rule(&mut self, rule: RoutingRule) {
        if let Some(existing) = self.rules.iter_mut().find(|r| r.id == rule.id) {
            *existing = rule;
        } else {
            self.rules.push(rule);
        }
    }

    /// Remove a routing rule by ID. Returns true if a rule was removed.
    pub fn remove_rule(&mut self, id: &str) -> bool {
        let len_before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() < len_before
    }

    /// Route a prompt to the best model.
    ///
    /// Rules are evaluated in priority order (descending). The first
    /// matching enabled rule is used. If no rule matches, the model
    /// with the highest priority is selected.
    ///
    /// # Arguments
    ///
    /// * `prompt` - The user's prompt text.
    /// * `file_count` - Number of files in the working context.
    /// * `complexity` - Estimated task complexity (0.0 to 1.0).
    ///
    /// # Returns
    ///
    /// A [`RoutingDecision`] with the selected model, reason, and confidence.
    pub fn route(&self, prompt: &str, file_count: usize, complexity: f64) -> RoutingDecision {
        // Get current hour for time-of-day conditions
        let current_hour = get_current_hour();

        // Sort rules by priority descending, then by ID for determinism
        let mut sorted_rules: Vec<&RoutingRule> = self.rules.iter().filter(|r| r.enabled).collect();
        sorted_rules.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.id.cmp(&b.id))
        });

        // Try each rule in priority order
        for rule in &sorted_rules {
            if rule.condition.matches(prompt, file_count, complexity, current_hour) {
                // Verify the target model exists
                if self.get_model(&rule.target_model).is_some() {
                    return RoutingDecision::new(
                        &rule.target_model,
                        &format!("Rule '{}': {}", rule.id, rule.description),
                        0.85,
                    );
                }
            }
        }

        // Fallback: select the model with the highest priority
        let default_model = self
            .models
            .iter()
            .max_by_key(|m| m.priority)
            .map(|m| &m.name)
            .cloned()
            .unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());

        RoutingDecision::new(
            &default_model,
            "No rules matched; using default highest-priority model",
            0.5,
        )
    }

    /// Get a model by name.
    pub fn get_model(&self, name: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|m| m.name == name)
    }

    /// List all registered models.
    pub fn list_models(&self) -> &[ModelInfo] {
        &self.models
    }

    /// List all routing rules.
    pub fn list_rules(&self) -> &[RoutingRule] {
        &self.rules
    }

    /// Set all rules at once (replaces existing rules).
    pub fn set_rules(&mut self, rules: Vec<RoutingRule>) {
        self.rules = rules;
    }
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the current hour of day (0-23) in local time.
fn get_current_hour() -> u32 {
    #[cfg(not(test))]
    {
        use chrono::{Local, Timelike};
        Local::now().hour()
    }
    #[cfg(test)]
    {
        // In tests, use a fixed hour to make tests deterministic
        12
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_router() -> ModelRouter {
        ModelRouter::new()
    }

    #[test]
    fn test_new_has_default_models() {
        let router = make_router();
        let models = router.list_models();
        assert_eq!(models.len(), 3);
        assert!(router.get_model("claude-sonnet-4-20250514").is_some());
        assert!(router.get_model("claude-opus-4-20250514").is_some());
        assert!(router.get_model("claude-3-5-haiku-20241022").is_some());
    }

    #[test]
    fn test_add_and_remove_model() {
        let mut router = make_router();
        let new_model = ModelInfo::new("test-model", "test", 100_000, 0.001, 0.005);
        router.add_model(new_model);
        assert_eq!(router.list_models().len(), 4);
        assert!(router.get_model("test-model").is_some());

        router.remove_model("test-model");
        assert_eq!(router.list_models().len(), 3);
        assert!(router.get_model("test-model").is_none());
    }

    #[test]
    fn test_add_model_replaces_existing() {
        let mut router = make_router();
        let updated = ModelInfo::new("claude-sonnet-4-20250514", "anthropic", 300_000, 0.002, 0.01)
            .with_priority(20);
        router.add_model(updated);
        let m = router.get_model("claude-sonnet-4-20250514").unwrap();
        assert_eq!(m.max_tokens, 300_000);
        assert_eq!(m.priority, 20);
        assert_eq!(router.list_models().len(), 3); // still 3
    }

    #[test]
    fn test_add_and_remove_rule() {
        let mut router = make_router();
        let rule = RoutingRule {
            id: "test-rule".into(),
            description: "Test".into(),
            condition: RouteCondition::Always,
            target_model: "claude-3-5-haiku-20241022".into(),
            priority: 10,
            enabled: true,
        };
        router.add_rule(rule);
        assert_eq!(router.list_rules().len(), 1);

        router.remove_rule("test-rule");
        assert_eq!(router.list_rules().len(), 0);
    }

    #[test]
    fn test_route_no_rules_uses_highest_priority() {
        let router = make_router();
        let decision = router.route("write a function", 0, 0.3);
        // Sonnet has priority 10, highest
        assert_eq!(decision.selected_model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_route_simple_task_to_haiku() {
        let mut router = make_router();
        router.add_rule(RoutingRule {
            id: "simple-to-haiku".into(),
            description: "Simple tasks use Haiku".into(),
            condition: RouteCondition::TaskComplexity {
                min: 0.0,
                max: 0.3,
            },
            target_model: "claude-3-5-haiku-20241022".into(),
            priority: 100,
            enabled: true,
        });
        let decision = router.route("translate hello to spanish", 0, 0.1);
        assert_eq!(decision.selected_model, "claude-3-5-haiku-20241022");
        assert!(decision.reason.contains("simple-to-haiku"));
    }

    #[test]
    fn test_route_complex_task_to_opus() {
        let mut router = make_router();
        router.add_rule(RoutingRule {
            id: "complex-to-opus".into(),
            description: "Complex tasks use Opus".into(),
            condition: RouteCondition::TaskComplexity {
                min: 0.7,
                max: 1.0,
            },
            target_model: "claude-opus-4-20250514".into(),
            priority: 100,
            enabled: true,
        });
        let decision = router.route("design a distributed system architecture", 10, 0.85);
        assert_eq!(decision.selected_model, "claude-opus-4-20250514");
    }

    #[test]
    fn test_route_file_count_condition() {
        let mut router = make_router();
        router.add_rule(RoutingRule {
            id: "many-files-to-opus".into(),
            description: "Many files → Opus".into(),
            condition: RouteCondition::FileCount { min: 5, max: 100 },
            target_model: "claude-opus-4-20250514".into(),
            priority: 100,
            enabled: true,
        });
        let decision = router.route("review these files", 8, 0.4);
        assert_eq!(decision.selected_model, "claude-opus-4-20250514");
    }

    #[test]
    fn test_route_message_length_condition() {
        let mut router = make_router();
        router.add_rule(RoutingRule {
            id: "long-to-opus".into(),
            description: "Long messages → Opus".into(),
            condition: RouteCondition::MessageLength {
                min: 500,
                max: 1_000_000,
            },
            target_model: "claude-opus-4-20250514".into(),
            priority: 100,
            enabled: true,
        });
        let long_prompt = "x".repeat(1000);
        let decision = router.route(&long_prompt, 0, 0.5);
        assert_eq!(decision.selected_model, "claude-opus-4-20250514");
    }

    #[test]
    fn test_route_rule_priority_ordering() {
        let mut router = make_router();
        // Lower priority rule that would match first
        router.add_rule(RoutingRule {
            id: "low-priority".into(),
            description: "Low priority".into(),
            condition: RouteCondition::Always,
            target_model: "claude-3-5-haiku-20241022".into(),
            priority: 1,
            enabled: true,
        });
        // Higher priority rule that also matches
        router.add_rule(RoutingRule {
            id: "high-priority".into(),
            description: "High priority".into(),
            condition: RouteCondition::Always,
            target_model: "claude-opus-4-20250514".into(),
            priority: 100,
            enabled: true,
        });
        let decision = router.route("anything", 0, 0.5);
        // The higher priority rule should win
        assert_eq!(decision.selected_model, "claude-opus-4-20250514");
    }

    #[test]
    fn test_route_disabled_rule_skipped() {
        let mut router = make_router();
        router.add_rule(RoutingRule {
            id: "disabled-rule".into(),
            description: "Should be skipped".into(),
            condition: RouteCondition::Always,
            target_model: "claude-3-5-haiku-20241022".into(),
            priority: 100,
            enabled: false,
        });
        let decision = router.route("anything", 0, 0.5);
        // Should fall through to default (highest priority model = sonnet)
        assert_eq!(decision.selected_model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_route_target_model_not_found_falls_through() {
        let mut router = make_router();
        router.add_rule(RoutingRule {
            id: "bad-target".into(),
            description: "Points to nonexistent model".into(),
            condition: RouteCondition::Always,
            target_model: "nonexistent-model".into(),
            priority: 100,
            enabled: true,
        });
        let decision = router.route("anything", 0, 0.5);
        // Should fall through to default
        assert_eq!(decision.selected_model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_set_rules() {
        let mut router = make_router();
        let rules = vec![
            RoutingRule {
                id: "r1".into(),
                description: "Rule 1".into(),
                condition: RouteCondition::Always,
                target_model: "claude-opus-4-20250514".into(),
                priority: 10,
                enabled: true,
            },
            RoutingRule {
                id: "r2".into(),
                description: "Rule 2".into(),
                condition: RouteCondition::Always,
                target_model: "claude-3-5-haiku-20241022".into(),
                priority: 5,
                enabled: true,
            },
        ];
        router.set_rules(rules);
        assert_eq!(router.list_rules().len(), 2);
    }
}
