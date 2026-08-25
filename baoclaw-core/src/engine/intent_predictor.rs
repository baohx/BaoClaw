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
    /// Targeted edit of existing code (small change, rename, tweak).
    CodeEdit,
    /// Searching within the codebase (grep / find symbol).
    Search,
    /// Image generation or editing.
    ImageGen,
    /// Web browsing / fetching a URL.
    WebBrowse,
    /// Deployment / DevOps operations.
    Deployment,
    /// General question / conversation.
    General,
}

impl UserIntent {
    /// Stable string key for an intent (used in configs/stats).
    pub fn key(&self) -> &'static str {
        match self {
            UserIntent::CodeWriting => "code_writing",
            UserIntent::Debugging => "debugging",
            UserIntent::CodeReview => "code_review",
            UserIntent::Refactoring => "refactoring",
            UserIntent::Testing => "testing",
            UserIntent::Documentation => "doc_write",
            UserIntent::FileManagement => "file_management",
            UserIntent::GitOps => "git_op",
            UserIntent::Research => "research",
            UserIntent::CodeEdit => "code_edit",
            UserIntent::Search => "search",
            UserIntent::ImageGen => "image",
            UserIntent::WebBrowse => "web",
            UserIntent::Deployment => "deployment",
            UserIntent::General => "general",
        }
    }

    /// Parse an intent from its string key.
    pub fn from_key(key: &str) -> UserIntent {
        match key {
            "code_writing" => UserIntent::CodeWriting,
            "debugging" | "debug" => UserIntent::Debugging,
            "code_review" => UserIntent::CodeReview,
            "refactoring" => UserIntent::Refactoring,
            "testing" => UserIntent::Testing,
            "doc_write" | "documentation" => UserIntent::Documentation,
            "file_management" => UserIntent::FileManagement,
            "git_op" | "git_ops" => UserIntent::GitOps,
            "research" => UserIntent::Research,
            "code_edit" => UserIntent::CodeEdit,
            "search" => UserIntent::Search,
            "image" => UserIntent::ImageGen,
            "web" => UserIntent::WebBrowse,
            "deployment" => UserIntent::Deployment,
            _ => UserIntent::General,
        }
    }
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

impl Default for IntentPredictor {
    fn default() -> Self {
        Self::new()
    }
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

        patterns.insert(
            "code_edit".into(),
            IntentPattern {
                keywords: vec![
                    "edit".into(),
                    "modify".into(),
                    "change".into(),
                    "update".into(),
                    "rename".into(),
                    "修改".into(),
                    "改一下".into(),
                ],
                tools_used: vec!["FileEdit".into(), "FileRead".into()],
                file_extensions: vec![".rs".into(), ".ts".into()],
                observation_count: 5,
                next_intent: Some(UserIntent::Testing),
                next_intent_prob: 0.4,
            },
        );

        patterns.insert(
            "search".into(),
            IntentPattern {
                keywords: vec![
                    "grep".into(),
                    "where is".into(),
                    "locate".into(),
                    "which file".into(),
                    "搜索".into(),
                    "哪个文件".into(),
                ],
                tools_used: vec!["Bash".into(), "FileRead".into()],
                file_extensions: vec![],
                observation_count: 5,
                next_intent: Some(UserIntent::CodeEdit),
                next_intent_prob: 0.5,
            },
        );

        patterns.insert(
            "doc_write".into(),
            IntentPattern {
                keywords: vec![
                    "document".into(),
                    "readme".into(),
                    "docs".into(),
                    "comment".into(),
                    "documentation".into(),
                    "文档".into(),
                    "注释".into(),
                ],
                tools_used: vec!["FileWrite".into(), "FileRead".into()],
                file_extensions: vec![".md".into()],
                observation_count: 5,
                next_intent: None,
                next_intent_prob: 0.0,
            },
        );

        patterns.insert(
            "image".into(),
            IntentPattern {
                keywords: vec![
                    "image".into(),
                    "picture".into(),
                    "draw".into(),
                    "generate an image".into(),
                    "logo".into(),
                    "图片".into(),
                    "画".into(),
                ],
                tools_used: vec!["ImageGenerator".into(), "ImageEditor".into()],
                file_extensions: vec![".png".into(), ".jpg".into()],
                observation_count: 3,
                next_intent: None,
                next_intent_prob: 0.0,
            },
        );

        patterns.insert(
            "web".into(),
            IntentPattern {
                keywords: vec![
                    "http://".into(),
                    "https://".into(),
                    "url".into(),
                    "website".into(),
                    "fetch".into(),
                    "browse".into(),
                    "网页".into(),
                ],
                tools_used: vec!["WebFetch".into(), "WebSearch".into()],
                file_extensions: vec![],
                observation_count: 3,
                next_intent: None,
                next_intent_prob: 0.0,
            },
        );

        patterns.insert(
            "deployment".into(),
            IntentPattern {
                keywords: vec![
                    "deploy".into(),
                    "docker".into(),
                    "kubernetes".into(),
                    "k8s".into(),
                    "release".into(),
                    "ci/cd".into(),
                    "部署".into(),
                    "发布".into(),
                ],
                tools_used: vec!["Bash".into()],
                file_extensions: vec![".yaml".into(), ".yml".into()],
                observation_count: 3,
                next_intent: None,
                next_intent_prob: 0.0,
            },
        );

        patterns.insert(
            "code_review".into(),
            IntentPattern {
                keywords: vec![
                    "review".into(),
                    "explain".into(),
                    "understand".into(),
                    "what does".into(),
                    "审查".into(),
                    "解释".into(),
                ],
                tools_used: vec!["FileRead".into()],
                file_extensions: vec![".rs".into(), ".ts".into()],
                observation_count: 3,
                next_intent: Some(UserIntent::CodeEdit),
                next_intent_prob: 0.3,
            },
        );

        patterns.insert(
            "file_management".into(),
            IntentPattern {
                keywords: vec![
                    "move".into(),
                    "copy".into(),
                    "delete file".into(),
                    "mkdir".into(),
                    "list files".into(),
                    "移动".into(),
                ],
                tools_used: vec!["Bash".into()],
                file_extensions: vec![],
                observation_count: 3,
                next_intent: None,
                next_intent_prob: 0.0,
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

    /// Score all patterns against a message. Returns (intent_key, score, tools) tuples.
    fn score_all(&self, user_message: &str) -> Vec<(String, f64, Vec<String>)> {
        let lower = user_message.to_lowercase();
        let mut scored = Vec::new();

        for (key, pattern) in &self.patterns {
            let mut score = 0.0;
            let mut matched_keywords = 0;

            for kw in &pattern.keywords {
                if lower.contains(kw.as_str()) {
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

            if score > 0.0 {
                scored.push((key.clone(), score, pattern.tools_used.clone()));
            }
        }

        // Sort descending by score
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Predict user intent from their message.
    pub fn predict(&mut self, user_message: &str) -> PredictedIntent {
        let scored = self.score_all(user_message);

        let (best_intent, best_score, best_tools) = scored
            .into_iter()
            .next()
            .map(|(key, score, tools)| (UserIntent::from_key(&key), score, tools))
            .unwrap_or((UserIntent::General, 0.0, Vec::new()));

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

    /// Predict multiple candidate intents with confidence scores, sorted descending.
    ///
    /// Returns up to `max_intents` predictions whose confidence exceeds `min_confidence`.
    /// Useful for warmup: a message like "fix the test failure" may map to both
    /// `debugging` and `testing` — warmup should consider both.
    pub fn predict_multi(
        &mut self,
        user_message: &str,
        max_intents: usize,
        min_confidence: f64,
    ) -> Vec<PredictedIntent> {
        let scored = self.score_all(user_message);
        self.prediction_count += 1;

        let mut out: Vec<PredictedIntent> = scored
            .into_iter()
            .map(|(key, score, tools)| {
                let confidence = score.min(1.0);
                let suggested_preloads = if confidence > 0.3 { tools } else { Vec::new() };
                PredictedIntent {
                    intent: UserIntent::from_key(&key),
                    confidence,
                    suggested_preloads,
                }
            })
            .filter(|p| p.confidence >= min_confidence)
            .take(max_intents)
            .collect();

        if out.is_empty() {
            out.push(PredictedIntent {
                intent: UserIntent::General,
                confidence: 0.0,
                suggested_preloads: Vec::new(),
            });
        }
        out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_key_roundtrip() {
        for intent in [
            UserIntent::CodeWriting,
            UserIntent::Debugging,
            UserIntent::CodeReview,
            UserIntent::Refactoring,
            UserIntent::Testing,
            UserIntent::Documentation,
            UserIntent::FileManagement,
            UserIntent::GitOps,
            UserIntent::Research,
            UserIntent::CodeEdit,
            UserIntent::Search,
            UserIntent::ImageGen,
            UserIntent::WebBrowse,
            UserIntent::Deployment,
            UserIntent::General,
        ] {
            assert_eq!(UserIntent::from_key(intent.key()), intent);
        }
    }

    #[test]
    fn test_predict_debugging() {
        let mut p = IntentPredictor::new();
        let pred = p.predict("there is a bug, the program crash with an error");
        assert_eq!(pred.intent, UserIntent::Debugging);
        assert!(pred.confidence > 0.0);
    }

    #[test]
    fn test_predict_new_categories() {
        let mut p = IntentPredictor::new();
        assert_eq!(
            p.predict("please deploy the app with docker and kubernetes").intent,
            UserIntent::Deployment
        );
        assert_eq!(
            p.predict("draw a picture of a logo image").intent,
            UserIntent::ImageGen
        );
        assert_eq!(
            p.predict("fetch https://example.com website").intent,
            UserIntent::WebBrowse
        );
    }

    #[test]
    fn test_predict_general_fallback() {
        let mut p = IntentPredictor::new();
        let pred = p.predict("你好");
        assert_eq!(pred.intent, UserIntent::General);
        assert_eq!(pred.confidence, 0.0);
        assert!(pred.suggested_preloads.is_empty());
    }

    #[test]
    fn test_predict_multi_returns_sorted() {
        let mut p = IntentPredictor::new();
        // Message touching both debugging and testing
        let preds = p.predict_multi("fix the bug in the test, it fails with an error", 3, 0.0);
        assert!(!preds.is_empty());
        assert!(preds.len() <= 3);
        // Sorted descending by confidence
        for w in preds.windows(2) {
            assert!(w[0].confidence >= w[1].confidence);
        }
        // Both debugging and testing should appear
        let intents: Vec<_> = preds.iter().map(|p| p.intent.clone()).collect();
        assert!(intents.contains(&UserIntent::Debugging));
        assert!(intents.contains(&UserIntent::Testing));
    }

    #[test]
    fn test_predict_multi_min_confidence() {
        let mut p = IntentPredictor::new();
        let preds = p.predict_multi("completely unrelated banana text", 3, 0.5);
        // Falls back to General with 0 confidence
        assert_eq!(preds.len(), 1);
        assert_eq!(preds[0].intent, UserIntent::General);
    }

    #[test]
    fn test_confidence_clamped() {
        let mut p = IntentPredictor::new();
        // Lots of keyword hits
        let pred = p.predict("error bug fix crash fail broken traceback panic");
        assert!(pred.confidence <= 1.0);
        assert!(pred.confidence > 0.5);
    }

    #[test]
    fn test_record_actual_updates_transitions() {
        let mut p = IntentPredictor::new();
        p.record_actual("debugging", &["Bash".to_string()]);
        p.record_actual("testing", &[]);
        assert_eq!(
            p.transitions.get(&("debugging".to_string(), "testing".to_string())),
            Some(&1)
        );
    }
}
