//! Accurate token counting via tiktoken + API calibration.
//!
//! Replaces the crude `chars / 4` estimator with a hybrid strategy:
//! 1. After each API call, the real `usage.input_tokens` is captured as a baseline.
//! 2. For subsequent estimates before the next API call, we use that baseline plus
//!    a tiktoken-based count of only the messages appended since.
//!
//! This keeps estimates accurate to within a single turn, vs the 4-8× error
//! that `chars / 4` produces on CJK-heavy contexts.

// Scaffold-stage allows: imports and helpers below land in subsequent tasks
// (calibrate/estimate/should_compact). Remove these once Tasks 2-3 fill in the
// methods that consume Arc/Mutex/Usage.
#![allow(dead_code)]
#![allow(unused_imports)]

use crate::models::message::{Message, MessageContent, Usage};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Tracks input-token usage per session, calibrated against real API responses.
#[derive(Debug)]
pub struct TokenCounter {
    /// Last known input token count from API (authoritative).
    last_known_input_tokens: Option<u64>,
    /// Message count at the time `last_known_input_tokens` was captured.
    last_known_message_count: usize,
    /// Auto-compact threshold as fraction of `context_window` (e.g. 0.7 = 70%).
    threshold_ratio: f64,
    /// Model's context window size in tokens (e.g. 200_000 for Claude).
    context_window: u64,
}

impl TokenCounter {
    /// Creates a new counter with no calibration baseline yet.
    pub fn new(context_window: u64, threshold_ratio: f64) -> Self {
        Self {
            last_known_input_tokens: None,
            last_known_message_count: 0,
            threshold_ratio,
            context_window,
        }
    }

    /// Flatten a message's content to plain text for tokenisation.
    /// Assistant messages are serialised as JSON so that tool_use blocks and
    /// tool args contribute their characters; user messages that are strings
    /// are passed through; system messages return their content verbatim.
    pub(crate) fn extract_text(msg: &Message) -> String {
        match &msg.content {
            MessageContent::User { message, .. } => {
                // User content is a JSON value — could be a string or a list
                // of content blocks (tool_results). Serialise either way.
                if let Some(s) = message.content.as_str() {
                    s.to_string()
                } else {
                    message.content.to_string()
                }
            }
            MessageContent::Assistant { message, .. } => {
                serde_json::to_string(&message.content).unwrap_or_default()
            }
            MessageContent::System { content, .. } => content.clone(),
            MessageContent::Progress { .. } => String::new(),
        }
    }

    /// Count tokens in a text string using the cl100k_base BPE tokeniser.
    /// cl100k is the tokeniser used by gpt-4/gpt-3.5. For Claude it over-counts
    /// by ~5-10%, which is still an order of magnitude more accurate than the
    /// previous `chars / 4` heuristic (which undercounted Chinese by 4-8×).
    /// Falls back to a `chars * 3 / 4` estimate if the tokeniser fails to load.
    pub(crate) fn count_text_tokens(text: &str) -> u64 {
        match tiktoken_rs::cl100k_base() {
            Ok(bpe) => bpe.encode_with_special_tokens(text).len() as u64,
            Err(_) => (text.chars().count() as u64).saturating_mul(3) / 4,
        }
    }

    /// Called after each API response to anchor the counter to a known value.
    /// `api_input_tokens` is the `usage.input_tokens` field from the response;
    /// `message_count_at_call` is `messages.len()` *at the moment that call
    /// was made* (i.e. before the assistant reply was appended).
    pub fn calibrate(&mut self, api_input_tokens: u64, message_count_at_call: usize) {
        self.last_known_input_tokens = Some(api_input_tokens);
        self.last_known_message_count = message_count_at_call;
    }

    /// Estimate the total input tokens for the given message list.
    /// Uses the most recent API baseline + tiktoken delta for messages added
    /// since that baseline. Without any baseline yet, falls back to full
    /// tiktoken-counting the entire message list.
    pub fn estimate(&self, messages: &[Message]) -> u64 {
        match self.last_known_input_tokens {
            Some(baseline) if messages.len() >= self.last_known_message_count => {
                let delta: u64 = messages[self.last_known_message_count..]
                    .iter()
                    .map(|m| Self::count_text_tokens(&Self::extract_text(m)))
                    .sum();
                baseline + delta
            }
            _ => messages
                .iter()
                .map(|m| Self::count_text_tokens(&Self::extract_text(m)))
                .sum(),
        }
    }

    /// Convenience accessor for the current estimate (matches API used elsewhere).
    pub fn current_estimate(&self, messages: &[Message]) -> u64 {
        self.estimate(messages)
    }

    /// Returns true when the estimated input tokens exceed
    /// `context_window * threshold_ratio` — the signal to auto-compact.
    pub fn should_compact(&self, messages: &[Message]) -> bool {
        let est = self.estimate(messages);
        let threshold = (self.context_window as f64 * self.threshold_ratio) as u64;
        est > threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::{ApiUserMessage, Message, MessageContent};

    fn user_msg(text: &str) -> Message {
        Message {
            uuid: "u1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            content: MessageContent::User {
                message: ApiUserMessage {
                    role: "user".to_string(),
                    content: serde_json::json!(text),
                },
                is_meta: false,
                tool_use_result: None,
            },
        }
    }

    #[test]
    fn test_new_counter_has_no_baseline() {
        let c = TokenCounter::new(200_000, 0.7);
        assert!(c.last_known_input_tokens.is_none());
        assert_eq!(c.last_known_message_count, 0);
    }

    #[test]
    fn test_extract_text_from_user_message() {
        let msg = user_msg("Hello world");
        let text = TokenCounter::extract_text(&msg);
        assert!(text.contains("Hello world"), "got: {}", text);
    }

    #[test]
    fn test_count_text_tokens_chinese() {
        // chars/4 would say "你好世界" ≈ 1. Tiktoken says 4-10.
        let n = TokenCounter::count_text_tokens("你好世界");
        assert!(n >= 4 && n <= 10, "got {}", n);
    }

    #[test]
    fn test_count_text_tokens_english() {
        // "Hello world" is 2 tokens in cl100k BPE.
        let n = TokenCounter::count_text_tokens("Hello world");
        assert_eq!(n, 2);
    }

    #[test]
    fn test_estimate_no_baseline_uses_full_tiktoken() {
        let c = TokenCounter::new(200_000, 0.7);
        let msgs = vec![user_msg("Hello world")];
        let est = c.estimate(&msgs);
        // No baseline → full count from scratch.
        // "Hello world" JSON-serialized ≈ 4 tokens; small upper bound.
        assert!(est >= 2 && est < 100, "got {}", est);
    }

    #[test]
    fn test_estimate_with_baseline_adds_delta() {
        let mut c = TokenCounter::new(200_000, 0.7);
        c.calibrate(12_000, 5); // API said 12k tokens at message #5
        let msgs: Vec<Message> = (0..7).map(|i| user_msg(&format!("msg {}", i))).collect();
        // 7 messages, baseline covers first 5, estimate adds messages 5 & 6
        let est = c.estimate(&msgs);
        assert!(est > 12_000, "must exceed baseline, got {}", est);
        assert!(est < 13_000, "delta should be tiny, got {}", est);
    }

    #[test]
    fn test_should_compact_triggers_at_threshold() {
        let mut c = TokenCounter::new(200_000, 0.7);
        c.calibrate(150_000, 0);
        // 150k > 200k * 0.7 = 140k → should compact
        assert!(c.should_compact(&[]));
    }

    #[test]
    fn test_should_compact_false_below_threshold() {
        let mut c = TokenCounter::new(200_000, 0.7);
        c.calibrate(100_000, 0);
        // 100k < 140k → no compact
        assert!(!c.should_compact(&[]));
    }
}
