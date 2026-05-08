//! Accurate token counting via tiktoken + API calibration.
//!
//! Replaces the crude `chars / 4` estimator with a hybrid strategy:
//! 1. After each API call, the real `usage.input_tokens` is captured as a baseline.
//! 2. For subsequent estimates before the next API call, we use that baseline plus
//!    a tiktoken-based count of only the messages appended since.
//!
//! This keeps estimates accurate to within a single turn, vs the 4-8× error
//! that `chars / 4` produces on CJK-heavy contexts.

#![allow(dead_code)]

use crate::models::message::{Message, MessageContent};

/// Tracks input-token usage per session, calibrated against real API responses.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_counter_has_no_baseline() {
        let c = TokenCounter::new(200_000, 0.7);
        assert!(c.last_known_input_tokens.is_none());
        assert_eq!(c.last_known_message_count, 0);
    }
}
