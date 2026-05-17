//! User intent prediction — learns patterns from interaction history to preload context.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A predicted user intent with confidence score.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PredictedIntent {
    /// Predicted intent category.
    pub intent: UserIntent,
    /// Confidence score (0.0-1.0).
    pub confidence: f64,
    /// Suggested preloads for this intent.
    pub suggested_preloads: Vec<String>,
}

/// User intent categories.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UserIntent {
    /// Writing new code / feature.
    CodeWriting,
    /// Debugging / fixing errors.
    Debugging,
    /// Reading / understanding code.
    CodeReview,
    /// Refactoring existing code.
    Refactoring,
    /// Writing tests.
    Testing,
    /// Documentation.
    Documentation,
    /// File management (search, move, rename).
    FileManagement,
    /// Git operations.
    GitOps,
    /// Web search / research.
    Research,
    /// General question / conversation.
    General,
}

/// Pattern learned from user interaction history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntentPattern {
    /// Keywords that trigger this intent.
    pub keywords: Vec<String>,
    /// Tools typically used for this intent.
    pub tools_used: Vec<String>,
    /// Files typically accessed for this intent (by extension).
    pub file_extensions: Vec<String>,
    /// How many times this pattern was observed.
    pub observation_count: u32,
    /// Transition: what intent typically follows this one.
    pub next_intent: Option<UserIntent>,
    /// Transition probability.
    pub next_intent_prob: f64,
}

/// Tracks intent patterns and makes predictions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntentPredictor {
    /// Learned patterns per intent.
    pub patterns: HashMap<String, IntentPattern>,
    /// Transition matrix: (intent_from, intent_to) -> count.
    pub transitions: HashMap<(String, String), u32>,
    /// Last predicted intent (for transition learning).
    pub last_intent: Option<String>,
    /// Total predictions made.
    pub prediction_count: u32,
    /// Correct predictions (user confirmed by following predicted path).
    pub correct_predictions: u32,
}

impl IntentPredictor {
    pub fn new() -> Self {
        let mut patterns = HashMap::new();

        // Seed with common patterns
        patterns.insert(
            "debugging".into(),
            IntentPattern {
                keywords: vec![
                    "error".into(),
                    "bug".into(),
                    "fix".into(),
                    "crash".into(),
                    "fail".into(),
                    "broken".into(),
                    "traceback".into(),
                    "panic".into(),
                ],
                tools_used: vec!["Bash".into(), "FileRead".into()],
                file_extensions: vec![".rs".into(), ".log".into()],
                observation_count: 10, // seeded
                next_intent: Some(UserIntent::CodeWriting),
                next_intent_prob: 0.6,
            },
        );

        patterns.insert(
            "code_writing".into(),
            IntentPattern {
                keywords: vec![
                    "create".into(),
                    "add".into(),
                    "implement".into(),
                    "write".into(),
                    "build".into(),
                    "new feature".into(),
                    "新增".into(),
                    "实现".into(),
                ],
                tools_used: vec!["FileEdit".into(), "FileWrite".into()],
                file_extensions: vec![".rs".into(), ".toml".into(), ".md".into()],
                observation_count: 10,
                next_intent: Some(UserIntent::Testing),
                next_intent_prob: 0.5,
            },
        );

        patterns.insert(
            "refactoring".into(),
            IntentPattern {
                keywords: vec![
                    "refactor".into(),
                    "restructure".into(),
                    "clean up".into(),
                    "simplify".into(),
                    "重构".into(),
                ],
                tools_used: vec!["FileEdit".into(), "FileRead".into()],
                file_extensions: vec![".rs".into()],
                observation_count: 5,
                next_intent: Some(UserIntent::Testing),
                next_intent_prob: 0.7,
            },
        );

        patterns.insert(
            "testing".into(),
            IntentPattern {
                keywords: vec![
                    "test".into(),
                    "verify".into(),
                    "check".into(),
                    "测试".into(),
                    "验证".into(),
                ],
                tools_used: vec!["Bash".into()],
                file_extensions: vec![".rs".into()],
                observation_count: 5,
                next_intent: Some(UserIntent::Debugging),
                next_intent_prob: 0.4,
            },
        );

        patterns.insert(
            "git_ops".into(),
            IntentPattern {
                keywords: vec![
                    "commit".into(),
                    "push".into(),
                    "merge".into(),
                    "branch".into(),
                    "pull request".into(),
                    "提交".into(),
                ],
                tools_used: vec!["Bash".into()],
                file_extensions: vec![],
                observation_count: 5,
                next_intent: None,
                next_intent_prob: 0.0,
            },
        );

        patterns.insert(
            "research".into(),
            IntentPattern {
                keywords: vec![
                    "search".into(),
                    "find".into(),
                    "lookup".into(),
                    "what is".into(),
                    "how to".into(),
                    "查找".into(),
                ],
                tools_used: vec!["WebSearch".into(), "WebFetch".into()],
                file_extensions: vec![],
                observation_count: 3,
                next_intent: Some(UserIntent::CodeWriting),
                next_intent_prob: 0.5,
            },
        );

        Self {
            patterns,
            transitions: HashMap::new(),
            last_intent: None,
            prediction_count: 0,
            correct_predictions: 0,
        }
    }

    /// Predict user intent from their message.
    pub fn predict(&mut self, user_message: &str) -> PredictedIntent {
        let lower = user_message.to_lowercase();
        let mut best_intent = UserIntent::General;
        let mut best_score = 0.0;
        let mut best_tools = Vec::new();

        for (key, pattern) in &self.patterns {
            let mut score = 0.0;
            let mut matched_keywords = 0;

            for kw in &pattern.keywords {
                if lower.contains(kw) {
                    matched_keywords += 1;
                }
            }

            if matched_keywords > 0 {
                // Score = keyword match ratio * log(observation_count)
                score = (matched_keywords as f64 / pattern.keywords.len().max(1) as f64)
                    * (1.0 + (pattern.observation_count as f64).ln().max(0.0) / 5.0);
            }

            // Boost if transition from last intent predicts this
            if let Some(ref last) = self.last_intent {
                let trans_key = (last.clone(), key.clone());
                if let Some(&count) = self.transitions.get(&trans_key) {
                    score += count as f64 * 0.1;
                }
            }

            if score > best_score {
                best_score = score;
                best_tools = pattern.tools_used.clone();
                best_intent = match key.as_str() {
                    "debugging" => UserIntent::Debugging,
                    "code_writing" => UserIntent::CodeWriting,
                    "refactoring" => UserIntent::Refactoring,
                    "testing" => UserIntent::Testing,
                    "git_ops" => UserIntent::GitOps,
                    "research" => UserIntent::Research,
                    "documentation" => UserIntent::Documentation,
                    "code_review" => UserIntent::CodeReview,
                    "file_management" => UserIntent::FileManagement,
                    _ => UserIntent::General,
                };
            }
        }

        // Clamp confidence
        let confidence = best_score.min(1.0);

        // Build preload suggestions
        let suggested_preloads = if confidence > 0.3 {
            best_tools
        } else {
            Vec::new()
        };

        self.prediction_count += 1;

        PredictedIntent {
            intent: best_intent,
            confidence,
            suggested_preloads,
        }
    }

    /// Record the actual intent (for learning).
    pub fn record_actual(&mut self, intent_name: &str, tools_used: &[String]) {
        // Update transition matrix
        if let Some(ref last) = self.last_intent {
            let key = (last.clone(), intent_name.to_string());
            *self.transitions.entry(key).or_insert(0) += 1;
        }

        // Update pattern
        if let Some(pattern) = self.patterns.get_mut(intent_name) {
            pattern.observation_count += 1;
            for tool in tools_used {
                if !pattern.tools_used.contains(tool) {
                    pattern.tools_used.push(tool.clone());
                }
            }
        }

        self.last_intent = Some(intent_name.to_string());
    }

    /// Record that a prediction was correct.
    pub fn record_correct_prediction(&mut self) {
        self.correct_predictions += 1;
    }

    /// Get prediction accuracy.
    pub fn accuracy(&self) -> f64 {
        if self.prediction_count == 0 {
            return 0.0;
        }
        self.correct_predictions as f64 / self.prediction_count as f64
    }

    /// Build a system prompt hint based on predicted intent.
    pub fn build_preload_hint(&self, prediction: &PredictedIntent) -> Option<String> {
        if prediction.confidence < 0.3 || prediction.suggested_preloads.is_empty() {
            return None;
        }

        let intent_str = format!("{:?}", prediction.intent);
        let tools_str = prediction.suggested_preloads.join(", ");
        Some(format!(
            "\n## Predicted User Intent\nIntent: {} (confidence: {:.0}%)\nPreloaded tools: {}\n",
            intent_str,
            prediction.confidence * 100.0,
            tools_str,
        ))
    }
}
