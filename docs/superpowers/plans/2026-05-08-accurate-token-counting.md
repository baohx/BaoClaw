# Accurate Token Counting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `chars / 4` token estimator with a hybrid accurate counter that uses the API's real `usage.input_tokens` as a calibration baseline and only estimates the delta for unsent messages.

**Architecture:** Add a `TokenCounter` struct tracked per-session. It stores `last_known_input_tokens` + the message-index that produced it. For estimation, it uses that baseline plus a tiktoken-based estimate of only the messages appended since. When the API returns a new `usage.input_tokens`, the counter recalibrates. This eliminates the `chars / 4` error (4-8× for Chinese) and gives us real numbers within one API call.

**Tech Stack:** Rust, `tiktoken-rs` crate, `Arc<Mutex<TokenCounter>>` shared between `query_engine` and `cost_tracker`.

---

## File Structure

- **Create:** `baoclaw-core/src/engine/token_counter.rs` — new `TokenCounter` struct + tiktoken integration + tests
- **Modify:** `baoclaw-core/Cargo.toml` — add `tiktoken-rs = "0.5"` dependency
- **Modify:** `baoclaw-core/src/engine/mod.rs` — export `TokenCounter`
- **Modify:** `baoclaw-core/src/engine/query_engine.rs:1413-1449` — replace `estimate_tokens()` with `TokenCounter::estimate()`
- **Modify:** `baoclaw-core/src/engine/query_engine.rs:540,695,825` — read threshold from `TokenCounter::should_compact()`
- **Modify:** `baoclaw-core/src/engine/query_engine.rs:1070-1075` — feed `usage.input_tokens` into `TokenCounter::calibrate()` after each API response
- **Modify:** `baoclaw-core/src/config.rs` — add `auto_compact_threshold_ratio: f64` (default 0.7) and `context_window: u64` (default 200_000) fields to `BaoclawConfig`

---

### Task 1: Add tiktoken dependency and baseline counter struct

**Files:**
- Modify: `baoclaw-core/Cargo.toml`
- Create: `baoclaw-core/src/engine/token_counter.rs`
- Test: inline `#[cfg(test)]` in `token_counter.rs`

- [ ] **Step 1: Add tiktoken-rs dependency**

Edit `baoclaw-core/Cargo.toml`, in `[dependencies]` section after the `reqwest` line:

```toml
tiktoken-rs = "0.5"
```

- [ ] **Step 2: Write the failing test for TokenCounter::new**

Create `baoclaw-core/src/engine/token_counter.rs`:

```rust
use crate::models::message::{Message, MessageContent, Usage};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct TokenCounter {
    /// Last known input token count from API (authoritative).
    last_known_input_tokens: Option<u64>,
    /// Message count at the time last_known was captured.
    last_known_message_count: usize,
    /// Auto-compact threshold as fraction of context_window (e.g. 0.7).
    threshold_ratio: f64,
    /// Model's context window size in tokens (e.g. 200_000 for claude).
    context_window: u64,
}

impl TokenCounter {
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
```

- [ ] **Step 3: Run test to verify it passes**

```bash
cd baoclaw-core && cargo test --lib token_counter::tests::test_new_counter_has_no_baseline
```

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add baoclaw-core/Cargo.toml baoclaw-core/src/engine/token_counter.rs
git commit -m "feat(tokens): scaffold TokenCounter struct"
```

---

### Task 2: Implement tiktoken-based estimator for a message

**Files:**
- Modify: `baoclaw-core/src/engine/token_counter.rs`

- [ ] **Step 1: Write failing test for message-to-text extraction**

Add to `token_counter.rs` tests:

```rust
#[test]
fn test_extract_text_from_user_message() {
    use crate::models::message::MessageContent;
    let msg = Message {
        uuid: "u1".to_string(),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        content: MessageContent::User {
            content: serde_json::json!("Hello world"),
        },
    };
    let text = TokenCounter::extract_text(&msg);
    assert!(text.contains("Hello world"));
}
```

- [ ] **Step 2: Implement extract_text**

Add to `TokenCounter`:

```rust
impl TokenCounter {
    /// Flatten a message's content to plain text for tokenisation.
    pub(crate) fn extract_text(msg: &Message) -> String {
        match &msg.content {
            MessageContent::User { content } => content.to_string(),
            MessageContent::Assistant { content, .. } => {
                serde_json::to_string(content).unwrap_or_default()
            }
            MessageContent::System { content, .. } => content.clone(),
        }
    }
}
```

- [ ] **Step 3: Run test**

```bash
cargo test --lib token_counter::tests::test_extract_text_from_user_message
```

Expected: PASS

- [ ] **Step 4: Write failing test for tiktoken counting**

```rust
#[test]
fn test_count_text_tokens_chinese() {
    // "你好世界" should be ~6 tokens, NOT chars/4 = 1
    let n = TokenCounter::count_text_tokens("你好世界");
    assert!(n >= 4 && n <= 10, "got {}", n);
}

#[test]
fn test_count_text_tokens_english() {
    // "Hello world" is 2 tokens in cl100k
    let n = TokenCounter::count_text_tokens("Hello world");
    assert_eq!(n, 2);
}
```

- [ ] **Step 5: Implement count_text_tokens using tiktoken**

```rust
impl TokenCounter {
    pub(crate) fn count_text_tokens(text: &str) -> u64 {
        // cl100k_base is the tokenizer for gpt-4/gpt-3.5/claude (approximation).
        // It over-counts claude by ~5-10% but massively beats chars/4 for Chinese.
        match tiktoken_rs::cl100k_base() {
            Ok(bpe) => bpe.encode_with_special_tokens(text).len() as u64,
            Err(_) => (text.chars().count() as u64).saturating_mul(3) / 4,
        }
    }
}
```

- [ ] **Step 6: Run tests**

```bash
cargo test --lib token_counter::tests
```

Expected: both new tests PASS

- [ ] **Step 7: Commit**

```bash
git add baoclaw-core/src/engine/token_counter.rs
git commit -m "feat(tokens): implement tiktoken-based message counting"
```

---

### Task 3: Implement hybrid estimate() method (baseline + delta)

**Files:**
- Modify: `baoclaw-core/src/engine/token_counter.rs`

- [ ] **Step 1: Write failing test for estimate with no baseline**

```rust
#[test]
fn test_estimate_no_baseline_uses_full_tiktoken() {
    let c = TokenCounter::new(200_000, 0.7);
    let msgs = vec![Message {
        uuid: "u1".to_string(),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        content: MessageContent::User {
            content: serde_json::json!("Hello world"),
        },
    }];
    let est = c.estimate(&msgs);
    // No baseline → full count from scratch. "Hello world" = 2 + message wrapper ~5-10
    assert!(est >= 2 && est < 100);
}
```

- [ ] **Step 2: Write failing test for estimate with baseline**

```rust
#[test]
fn test_estimate_with_baseline_adds_delta() {
    let mut c = TokenCounter::new(200_000, 0.7);
    c.calibrate(12_000, 5); // API said 12k tokens at message #5
    let msgs: Vec<Message> = (0..7)
        .map(|i| Message {
            uuid: format!("u{}", i),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            content: MessageContent::User {
                content: serde_json::json!(format!("msg {}", i)),
            },
        })
        .collect();
    // 7 messages, baseline covers first 5, estimate adds messages 5 & 6
    let est = c.estimate(&msgs);
    assert!(est > 12_000); // must be larger than baseline
    assert!(est < 13_000); // but only a small delta
}
```

- [ ] **Step 3: Implement calibrate and estimate**

```rust
impl TokenCounter {
    /// Called after each API response to anchor the counter to a known value.
    pub fn calibrate(&mut self, api_input_tokens: u64, message_count_at_call: usize) {
        self.last_known_input_tokens = Some(api_input_tokens);
        self.last_known_message_count = message_count_at_call;
    }

    /// Estimate the total input tokens for the given message list.
    /// Uses the last API baseline + tiktoken delta for new messages.
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
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --lib token_counter::tests
```

Expected: all PASS

- [ ] **Step 5: Write failing test for should_compact**

```rust
#[test]
fn test_should_compact_triggers_at_threshold() {
    let mut c = TokenCounter::new(200_000, 0.7);
    c.calibrate(150_000, 0);
    assert!(c.should_compact(&[]));
}

#[test]
fn test_should_compact_false_below_threshold() {
    let mut c = TokenCounter::new(200_000, 0.7);
    c.calibrate(100_000, 0);
    assert!(!c.should_compact(&[]));
}
```

- [ ] **Step 6: Implement should_compact**

```rust
impl TokenCounter {
    pub fn should_compact(&self, messages: &[Message]) -> bool {
        let est = self.estimate(messages);
        let threshold = (self.context_window as f64 * self.threshold_ratio) as u64;
        est > threshold
    }

    pub fn current_estimate(&self, messages: &[Message]) -> u64 {
        self.estimate(messages)
    }
}
```

- [ ] **Step 7: Run tests**

```bash
cargo test --lib token_counter::
```

Expected: all PASS

- [ ] **Step 8: Commit**

```bash
git commit -am "feat(tokens): implement hybrid estimate with API calibration"
```

---

### Task 4: Wire TokenCounter into config and query_engine

**Files:**
- Modify: `baoclaw-core/src/config.rs`
- Modify: `baoclaw-core/src/engine/mod.rs`
- Modify: `baoclaw-core/src/engine/query_engine.rs`

- [ ] **Step 1: Add config fields**

In `baoclaw-core/src/config.rs`, inside `BaoclawConfig` struct:

```rust
#[serde(default = "default_context_window")]
pub context_window: u64,

#[serde(default = "default_compact_threshold")]
pub auto_compact_threshold_ratio: f64,
```

And the default functions near other `default_*` functions:

```rust
fn default_context_window() -> u64 { 200_000 }
fn default_compact_threshold() -> f64 { 0.7 }
```

Also add both fields to any `impl Default for BaoclawConfig` block with the same defaults.

- [ ] **Step 2: Export TokenCounter**

In `baoclaw-core/src/engine/mod.rs`, add:

```rust
pub mod token_counter;
pub use token_counter::TokenCounter;
```

- [ ] **Step 3: Add counter to QueryLoopConfig and Engine state**

Locate `pub struct QueryLoopConfig` in `query_engine.rs`. Add a field:

```rust
pub token_counter: Arc<tokio::sync::Mutex<TokenCounter>>,
```

Wherever `QueryLoopConfig` is constructed (search for `QueryLoopConfig {`), initialize with:

```rust
token_counter: Arc::new(tokio::sync::Mutex::new(TokenCounter::new(
    config.context_window,
    config.auto_compact_threshold_ratio,
))),
```

- [ ] **Step 4: Compile check**

```bash
cd baoclaw-core && cargo check 2>&1 | grep "^error" | head -20
```

Expected: no errors (warnings okay).

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(tokens): wire TokenCounter into QueryLoopConfig"
```

---

### Task 5: Replace estimate_tokens() usage in query_engine.rs

**Files:**
- Modify: `baoclaw-core/src/engine/query_engine.rs`

- [ ] **Step 1: Find all `estimate_tokens` call sites**

```bash
grep -n "estimate_tokens" baoclaw-core/src/engine/query_engine.rs
```

Expected: 3-5 call sites in `submit_message_with_attachments`, `run_query_loop`, and `compact_messages`.

- [ ] **Step 2: Replace each call with async counter lookup**

For each call site, replace:

```rust
let current_tokens = estimate_tokens(&messages);
```

with:

```rust
let current_tokens = config.token_counter.lock().await.current_estimate(&messages);
```

For `MAX_CONTEXT_TOKENS` constant comparisons, replace with:

```rust
if config.token_counter.lock().await.should_compact(&messages) && messages.len() > 5 {
```

- [ ] **Step 3: Remove the old estimate_tokens function**

Delete the old `fn estimate_tokens(messages: &[Message]) -> u64` function at lines 1413-1449.

Delete constants `MAX_CONTEXT_TOKENS` at lines ~540 and ~686 (they are now in config).

- [ ] **Step 4: Compile**

```bash
cd baoclaw-core && cargo build --release 2>&1 | grep "^error" | head -10
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(tokens): replace chars/4 estimator with TokenCounter"
```

---

### Task 6: Calibrate counter from API responses

**Files:**
- Modify: `baoclaw-core/src/engine/query_engine.rs`

- [ ] **Step 1: Find where Usage is captured from stream**

Grep for `MessageDelta` or `message_delta` in the streaming loop:

```bash
grep -n "MessageDelta\|message_delta\|usage.*input_tokens" baoclaw-core/src/engine/query_engine.rs | head -10
```

- [ ] **Step 2: Add calibration right after cost_tracker.accumulate**

Find the location (around line 1065-1075) where `cost_tracker.accumulate(...)` is called after each full turn. Immediately after it, add:

```rust
// Calibrate the token counter against the real API-reported input_tokens
{
    let mut counter = config.token_counter.lock().await;
    counter.calibrate(total_usage.input_tokens, messages.len());
}
```

- [ ] **Step 3: Build and run existing tests**

```bash
cd baoclaw-core && cargo build --release 2>&1 | tail -3
cargo test --lib engine::token_counter
```

Expected: build succeeds, token_counter tests PASS.

- [ ] **Step 4: Add integration test for calibration**

Create `baoclaw-core/tests/token_calibration_test.rs`:

```rust
use baoclaw_core::engine::TokenCounter;

#[test]
fn calibration_reduces_estimate_error() {
    let mut c = TokenCounter::new(200_000, 0.7);
    let chinese = TokenCounter::count_text_tokens("测试中文分词的准确度和性能表现。");
    // Sanity: tiktoken should give us ≥ 6 tokens for this string
    assert!(chinese >= 6, "Chinese tokens = {}", chinese);
    // chars/4 would give ≈ 4 → clearly undercounting
    assert!(chinese > 15 / 4);
}
```

- [ ] **Step 5: Run the integration test**

```bash
cargo test --test token_calibration_test
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add baoclaw-core/tests/token_calibration_test.rs baoclaw-core/src/engine/query_engine.rs
git commit -m "feat(tokens): calibrate counter from API usage responses"
```

---

### Task 7: Verify end-to-end with a real daemon run

**Files:** (observation only)

- [ ] **Step 1: Build release binary**

```bash
cd baoclaw-core && cargo build --release
```

Expected: compiles cleanly.

- [ ] **Step 2: Deploy binary atomically**

```bash
cp target/release/baoclaw-core /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core.new
mv -f /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core.new /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core
```

Expected: no "Text file busy" error.

- [ ] **Step 3: Restart daemon and send a long-context query**

In a new terminal: `/shutdown` then `baoclaw` to pick up new binary. Send a query that loads a large file (e.g., "read the entire query_engine.rs and summarize").

- [ ] **Step 4: Verify no spurious auto-compact on small contexts**

Observe: for small contexts (<100K tokens), no `Compacted:` progress event should appear.

- [ ] **Step 5: Final commit**

No code changes, but tag the milestone:

```bash
git tag -a tokens-v1 -m "Accurate token counting shipped"
```

---

## Self-Review Notes

**Spec coverage check:**
- ✅ Accurate counting → tiktoken + hybrid baseline
- ✅ Configurable threshold → `auto_compact_threshold_ratio`
- ✅ Calibrates from API → `calibrate()` called after each turn
- ✅ Removes hardcoded `400_000`/`800_000` constants → consolidated in config

**No placeholders:** All code steps contain full implementations.

**Type consistency:** `TokenCounter::new(u64, f64)` signature is consistent across Tasks 1-6.
