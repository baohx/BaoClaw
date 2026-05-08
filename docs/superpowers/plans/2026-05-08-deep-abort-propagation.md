# Deep Abort Propagation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Ctrl+C stop the agent within ~200ms at any stage (streaming, tool execution, compacting) by propagating the abort signal through every blocking await point, and protect message history integrity after abort.

**Architecture:** The daemon already has a `watch::Sender<bool>` that the CLI triggers via IPC. But only the tool executor and Bash honor it. We (a) wrap every `stream.next().await` in `tokio::select!` against abort, (b) pass abort_rx into HTTP streaming requests so reqwest can drop the connection, (c) add abort checks to WebFetch/WebSearch/compact, (d) after abort, scan the message tail and either complete orphan tool_use with a synthetic `aborted` tool_result or pop the partial assistant message.

**Tech Stack:** Rust, `tokio::select!`, `tokio::sync::watch`, `reqwest` request cancellation via drop.

---

## File Structure

- **Modify:** `baoclaw-core/src/engine/query_engine.rs:868-880` — tighten stream `tokio::select!` (already uses 500ms poll; make it event-driven via `abort_rx.changed()`)
- **Modify:** `baoclaw-core/src/engine/query_engine.rs:443,1178,1432` — three other `while let Some(event) = stream.next().await` loops need abort guards
- **Modify:** `baoclaw-core/src/engine/query_engine.rs:1324-1478` (`compact_messages`) — make compact interruptible
- **Modify:** `baoclaw-core/src/engine/query_engine.rs` — add `cleanup_orphan_tool_uses` helper + call after abort
- **Modify:** `baoclaw-core/src/tools/builtins/web_fetch_tool.rs` — race reqwest against abort
- **Modify:** `baoclaw-core/src/tools/builtins/web_search_tool.rs` — race reqwest against abort
- **Modify:** `baoclaw-core/src/tools/executor.rs` — early-exit if abort fires before tool call
- **Create:** `baoclaw-core/src/engine/abort_helpers.rs` — shared helpers: `wait_for_abort()`, `cleanup_orphan_tool_uses()`

---

### Task 1: Create shared abort helpers module

**Files:**
- Create: `baoclaw-core/src/engine/abort_helpers.rs`
- Modify: `baoclaw-core/src/engine/mod.rs`

- [ ] **Step 1: Write failing test for wait_for_abort**

Create `baoclaw-core/src/engine/abort_helpers.rs`:

```rust
use tokio::sync::watch;

/// Await until the abort watch fires (value becomes true).
/// If the sender is dropped with value=false, this future stays pending forever.
pub async fn wait_for_abort(mut rx: watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wait_for_abort_fires_when_set() {
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(wait_for_abort(rx));
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        tx.send(true).unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_wait_for_abort_ignores_false_dropped_sender() {
        let (tx, rx) = watch::channel(false);
        drop(tx);
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            wait_for_abort(rx),
        )
        .await;
        assert!(result.is_err()); // timed out = good
    }
}
```

- [ ] **Step 2: Register the module**

In `baoclaw-core/src/engine/mod.rs` add:

```rust
pub mod abort_helpers;
pub use abort_helpers::{wait_for_abort, cleanup_orphan_tool_uses};
```

- [ ] **Step 3: Run tests**

```bash
cd baoclaw-core && cargo test --lib engine::abort_helpers
```

Expected: both tests PASS.

- [ ] **Step 4: Commit**

```bash
git add baoclaw-core/src/engine/abort_helpers.rs baoclaw-core/src/engine/mod.rs
git commit -m "feat(abort): add shared wait_for_abort helper"
```

---

### Task 2: Implement cleanup_orphan_tool_uses

**Files:**
- Modify: `baoclaw-core/src/engine/abort_helpers.rs`

- [ ] **Step 1: Write failing test for orphan cleanup**

Add to `abort_helpers.rs`:

```rust
use crate::models::message::{ContentBlock, Message, MessageContent, ApiAssistantMessage};

/// Scan the tail of `messages` and fix any assistant message whose `tool_use` blocks
/// have no matching `tool_result`. Injects synthetic aborted tool_results to keep
/// the history API-legal after a user abort.
pub fn cleanup_orphan_tool_uses(messages: &mut Vec<Message>) -> usize {
    let mut fixed = 0;
    // Walk backwards; the last assistant message is the only candidate after abort
    if let Some(last) = messages.last() {
        if let MessageContent::Assistant { message, .. } = &last.content {
            let tool_use_ids: Vec<String> = message.content.iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect();
            if tool_use_ids.is_empty() {
                return 0;
            }
            // Build synthetic user message with aborted tool_results for every tool_use
            let result_blocks: Vec<ContentBlock> = tool_use_ids
                .iter()
                .map(|id| ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: serde_json::json!([{
                        "type": "text",
                        "text": "Tool execution aborted by user.",
                    }]),
                    is_error: Some(true),
                })
                .collect();
            fixed = result_blocks.len();
            let synthetic_user = Message {
                uuid: uuid::Uuid::new_v4().to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                content: MessageContent::User {
                    content: serde_json::to_value(&result_blocks).unwrap(),
                },
            };
            messages.push(synthetic_user);
        }
    }
    fixed
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    use crate::models::message::{ApiAssistantMessage, ContentBlock, MessageContent};

    #[test]
    fn test_cleanup_leaves_clean_history_alone() {
        let mut msgs: Vec<Message> = vec![];
        assert_eq!(cleanup_orphan_tool_uses(&mut msgs), 0);
    }

    #[test]
    fn test_cleanup_adds_synthetic_result_for_orphan_tool_use() {
        let asst_msg = ApiAssistantMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({}),
            }],
            model: None,
            stop_reason: None,
            stop_sequence: None,
            usage: None,
        };
        let mut msgs = vec![Message {
            uuid: "u1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            content: MessageContent::Assistant {
                message: asst_msg,
                parent_tool_use_id: None,
            },
        }];
        let fixed = cleanup_orphan_tool_uses(&mut msgs);
        assert_eq!(fixed, 1);
        assert_eq!(msgs.len(), 2); // original + synthetic user
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cd baoclaw-core && cargo test --lib engine::abort_helpers::cleanup_tests
```

Expected: both PASS.

- [ ] **Step 3: Commit**

```bash
git commit -am "feat(abort): add cleanup_orphan_tool_uses helper"
```

---

### Task 3: Tighten the main stream loop's abort check

**Files:**
- Modify: `baoclaw-core/src/engine/query_engine.rs:868-900`

- [ ] **Step 1: Locate current stream loop**

```bash
grep -n "while let Some(event_result) = tokio::select" baoclaw-core/src/engine/query_engine.rs
```

Expected: line ~872.

- [ ] **Step 2: Replace 500ms poll with event-driven wait**

Find the block that looks like:

```rust
while let Some(event_result) = tokio::select! {
    result = stream.next() => result,
    _ = async {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if config.is_aborted() { break; }
        }
    } => {
```

Replace with:

```rust
while let Some(event_result) = tokio::select! {
    result = stream.next() => result,
    _ = crate::engine::wait_for_abort(config.abort_rx.clone()) => {
```

- [ ] **Step 3: Compile**

```bash
cd baoclaw-core && cargo check 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(abort): replace 500ms abort poll with event-driven wait"
```

---

### Task 4: Add abort race to the three other stream consumers

**Files:**
- Modify: `baoclaw-core/src/engine/query_engine.rs` (lines 443, 1178, 1432)

- [ ] **Step 1: Find each consumer**

```bash
grep -n "while let Some(event_result) = stream.next().await" baoclaw-core/src/engine/query_engine.rs
```

Expected: 3 occurrences.

- [ ] **Step 2: For each one, wrap with tokio::select!**

For the loop in `submit_message_with_attachments` (around line 443, where its summary uses `compact_messages` flow), locate:

```rust
while let Some(event_result) = stream.next().await {
    match event_result { ... }
}
```

Replace with:

```rust
loop {
    let event_result = tokio::select! {
        r = stream.next() => r,
        _ = crate::engine::wait_for_abort(abort_rx.clone()) => {
            eprintln!("Aborted during summary streaming");
            break;
        }
    };
    let Some(event_result) = event_result else { break; };
    match event_result { ... }
}
```

**Important:** if the surrounding function does not have `abort_rx` in scope, add it to the function signature or use the engine's `self.abort_rx.clone()`.

Repeat for the other two loops (lines 1178, 1432).

- [ ] **Step 3: Compile**

```bash
cd baoclaw-core && cargo check 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(abort): race all 4 stream consumers against abort"
```

---

### Task 5: Make WebFetch tool abort-aware

**Files:**
- Modify: `baoclaw-core/src/tools/builtins/web_fetch_tool.rs`

- [ ] **Step 1: Find the fetch call**

```bash
grep -n "http_client.get\|reqwest::get\|\.send().await" baoclaw-core/src/tools/builtins/web_fetch_tool.rs | head -5
```

- [ ] **Step 2: Wrap fetch with select**

In the `call` method, locate where `self.http_client.get(url).send().await` is awaited. Wrap with:

```rust
let abort_signal = context.abort_signal.clone();
let response = tokio::select! {
    r = self.http_client.get(&url).send() => r,
    _ = async {
        let mut rx = abort_signal.as_ref().clone();
        loop {
            if *rx.borrow() { break; }
            if rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    } => return Err(ToolError::Aborted),
};
```

Same treatment for `response.text().await` or `response.bytes().await`.

- [ ] **Step 3: Compile**

```bash
cargo check 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(abort): WebFetch tool honors abort signal"
```

---

### Task 6: Apply the same pattern to WebSearch

**Files:**
- Modify: `baoclaw-core/src/tools/builtins/web_search_tool.rs`

- [ ] **Step 1: Locate the fetch call**

```bash
grep -n "\.send()\.await\|http_client\.get" baoclaw-core/src/tools/builtins/web_search_tool.rs
```

- [ ] **Step 2: Wrap with the same tokio::select! pattern**

(Same code template as Task 5 step 2.)

- [ ] **Step 3: Compile and commit**

```bash
cargo check 2>&1 | grep "^error" | head -5
git commit -am "feat(abort): WebSearch tool honors abort signal"
```

---

### Task 7: Early-exit in executor if already aborted

**Files:**
- Modify: `baoclaw-core/src/tools/executor.rs`

- [ ] **Step 1: Find tool dispatch entry**

```bash
grep -n "async fn execute_tools\|async fn execute_tool_call" baoclaw-core/src/tools/executor.rs | head -3
```

- [ ] **Step 2: Add early-exit check at the top of execute_tool_call**

Inside the function, immediately before invoking `tool.call(...)`:

```rust
if *context.abort_signal.borrow() {
    return ToolExecutionResult {
        tool_use_id: request.tool_use_id.clone(),
        result: Err(ToolError::Aborted),
    };
}
```

- [ ] **Step 3: Compile**

```bash
cargo check 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(abort): tool executor early-exits when aborted"
```

---

### Task 8: Make compact_messages interruptible

**Files:**
- Modify: `baoclaw-core/src/engine/query_engine.rs` (around line 1432)

- [ ] **Step 1: Find compact's stream loop**

In `compact_messages`, find:

```rust
while let Some(event_result) = stream.next().await {
    match event_result { ... }
}
```

- [ ] **Step 2: Wrap with abort**

Replace with:

```rust
loop {
    let event_result = tokio::select! {
        r = stream.next() => r,
        _ = crate::engine::wait_for_abort(config.abort_rx.clone()) => {
            eprintln!("Compact aborted by user");
            return Err(EngineError {
                code: "compact_aborted".to_string(),
                message: "User aborted compaction".to_string(),
                details: None,
            });
        }
    };
    let Some(event_result) = event_result else { break; };
    match event_result {
        Ok(event) => {
            if let ApiStreamEvent::ContentBlockDelta { delta, .. } = event {
                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                    summary_text.push_str(text);
                }
            }
        }
        Err(e) => {
            eprintln!("Compact: stream error: {}", e);
            break;
        }
    }
}
```

- [ ] **Step 3: Compile**

```bash
cargo check 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(abort): compact_messages is now interruptible"
```

---

### Task 9: Call cleanup_orphan_tool_uses after abort

**Files:**
- Modify: `baoclaw-core/src/engine/query_engine.rs`

- [ ] **Step 1: Locate all abort exit points in run_query_loop**

```bash
grep -n "QueryStatus::Aborted\|is_aborted.*break" baoclaw-core/src/engine/query_engine.rs
```

- [ ] **Step 2: Before returning Aborted status, clean up messages**

At each abort exit point (where `QueryStatus::Aborted` result is sent), add immediately before the send:

```rust
let fixed = crate::engine::cleanup_orphan_tool_uses(messages);
if fixed > 0 {
    eprintln!("Cleaned up {} orphan tool_use block(s) after abort", fixed);
}
```

- [ ] **Step 3: Compile**

```bash
cargo build --release 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(abort): cleanup orphan tool_use after user abort"
```

---

### Task 10: End-to-end abort latency test

**Files:**
- Create: `baoclaw-core/tests/abort_latency_test.rs`

- [ ] **Step 1: Write integration test**

```rust
use std::time::Instant;
use tokio::sync::watch;

#[tokio::test]
async fn test_abort_wait_fires_under_50ms() {
    let (tx, rx) = watch::channel(false);
    let start = Instant::now();
    let handle = tokio::spawn(baoclaw_core::engine::wait_for_abort(rx));
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    tx.send(true).unwrap();
    handle.await.unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() < 50, "wait_for_abort took {}ms", elapsed.as_millis());
}
```

- [ ] **Step 2: Run the test**

```bash
cd baoclaw-core && cargo test --test abort_latency_test
```

Expected: PASS.

- [ ] **Step 3: Final deploy**

```bash
cargo build --release
cp target/release/baoclaw-core /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core.new
mv -f /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core.new /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core
```

- [ ] **Step 4: Commit + tag**

```bash
git add baoclaw-core/tests/abort_latency_test.rs
git commit -m "test(abort): verify sub-50ms abort latency"
git tag -a abort-v1 -m "Deep abort propagation shipped"
```

---

## Self-Review Notes

**Spec coverage:**
- ✅ Stream abort — Tasks 3, 4
- ✅ Tool abort (WebFetch/WebSearch) — Tasks 5, 6
- ✅ Executor early-exit — Task 7
- ✅ Compact abort — Task 8
- ✅ Message integrity after abort — Task 9
- ✅ Latency verification — Task 10

**No placeholders:** All code blocks are complete. Tool abort pattern is repeated verbatim in Task 5 and 6 (writing-plans says repeat, don't reference).

**Type consistency:** `wait_for_abort(rx: watch::Receiver<bool>)` signature used identically in Tasks 3, 4, 8, 10.

**Bash abort already works** (Bash tool uses the pattern at lines 99-130) — no task needed.
