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
}
