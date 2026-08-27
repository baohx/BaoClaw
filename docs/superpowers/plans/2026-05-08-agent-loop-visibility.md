# Agent Loop Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the CLI render agent activity as a nested, turn-grouped tree with token/cost meters, so users can see exactly what main vs sub-agent are doing without information overload.

**Architecture:** Add two new `EngineEvent` variants (`TurnStart`, `TurnEnd`) emitted from the query loop. AgentTool's sub-engine inherits a parent turn ID. The CLI maintains a stack of active turns and indents each event by the stack depth. A status line at each TurnEnd shows tool count, duration, and cumulative tokens.

**Tech Stack:** Rust (engine events), TypeScript (CLI rendering), Unicode box-drawing characters for nesting visuals.

---

## File Structure

- **Modify:** `baoclaw-core/src/engine/query_engine.rs` — add `TurnStart`/`TurnEnd` variants to `EngineEvent`; emit them in `run_query_loop`
- **Modify:** `baoclaw-core/src/tools/builtins/agent_tool.rs` — pass parent turn id, prefix events with parent context
- **Modify:** `ts-ipc/cli.ts` — add turn stack, render box-drawing borders, status footer

---

### Task 1: Add TurnStart/TurnEnd variants to EngineEvent

**Files:**

- Modify: `baoclaw-core/src/engine/query_engine.rs`

- [ ] **Step 1: Find EngineEvent enum**

```bash
grep -n "pub enum EngineEvent" baoclaw-core/src/engine/query_engine.rs
```

Expected: line ~59.

- [ ] **Step 2: Add the two variants**

In the `EngineEvent` enum (right before the closing `}` of the enum), add:

```rust
    #[serde(rename = "turn_start")]
    TurnStart {
        turn_id: u32,
        parent_turn_id: Option<u32>,
        agent_label: Option<String>,
    },
    #[serde(rename = "turn_end")]
    TurnEnd {
        turn_id: u32,
        duration_ms: u64,
        tool_count: u32,
        input_tokens: u64,
        output_tokens: u64,
    },
```

- [ ] **Step 3: Compile**

```bash
cd baoclaw-core && cargo check 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(events): add TurnStart and TurnEnd event variants"
```

---

### Task 2: Emit TurnStart/TurnEnd from run_query_loop

**Files:**

- Modify: `baoclaw-core/src/engine/query_engine.rs`

- [ ] **Step 1: Locate the turn boundary in run_query_loop**

```bash
grep -n "turn_count += 1\|turn_count >= max" baoclaw-core/src/engine/query_engine.rs
```

Expected: turn counter is updated near the top/bottom of the main loop.

- [ ] **Step 2: Add turn-id thread through QueryLoopConfig**

Find `pub struct QueryLoopConfig` and add:

```rust
pub parent_turn_id: Option<u32>,
pub agent_label: Option<String>,
```

Initialize them as `None` in every constructor of `QueryLoopConfig`.

- [ ] **Step 3: Track per-turn metrics**

At the very top of `run_query_loop`, just after `let mut turn_count = 0u32;`, add:

```rust
let mut turn_input_tokens_at_start: u64 = 0;
let mut turn_output_tokens_at_start: u64 = 0;
let mut turn_tool_count: u32 = 0;
let mut turn_start_time: std::time::Instant = std::time::Instant::now();
let mut turn_id_counter: u32 = 0;
```

- [ ] **Step 4: Emit TurnStart at the top of each loop iteration**

Find the spot where each turn begins (right after the abort check, before the API call). Add:

```rust
turn_id_counter += 1;
turn_start_time = std::time::Instant::now();
turn_tool_count = 0;
turn_input_tokens_at_start = total_usage.input_tokens;
turn_output_tokens_at_start = total_usage.output_tokens;
let _ = tx.send(EngineEvent::TurnStart {
    turn_id: turn_id_counter,
    parent_turn_id: config.parent_turn_id,
    agent_label: config.agent_label.clone(),
}).await;
```

- [ ] **Step 5: Emit TurnEnd at the bottom of each turn**

Find the place where the turn finishes (after tool results processed, before going back to top of loop). Add:

```rust
let _ = tx.send(EngineEvent::TurnEnd {
    turn_id: turn_id_counter,
    duration_ms: turn_start_time.elapsed().as_millis() as u64,
    tool_count: turn_tool_count,
    input_tokens: total_usage.input_tokens.saturating_sub(turn_input_tokens_at_start),
    output_tokens: total_usage.output_tokens.saturating_sub(turn_output_tokens_at_start),
}).await;
```

- [ ] **Step 6: Increment tool count on each tool**

Find where `EngineEvent::ToolUse` is sent. Right before the `tx.send(EngineEvent::ToolUse...)`, add:

```rust
turn_tool_count += 1;
```

- [ ] **Step 7: Compile**

```bash
cargo check 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git commit -am "feat(events): emit TurnStart/TurnEnd around each agent turn"
```

---

### Task 3: AgentTool propagates parent turn id

**Files:**

- Modify: `baoclaw-core/src/tools/builtins/agent_tool.rs`

- [ ] **Step 1: Find where AgentTool spawns its sub-engine**

```bash
grep -n "QueryLoopConfig\|run_query_loop\|parent_tool_use_id" baoclaw-core/src/tools/builtins/agent_tool.rs | head -10
```

- [ ] **Step 2: Pass parent_turn_id and agent_label**

In the `call` method, when constructing the sub-`QueryEngine` config, set:

```rust
parent_turn_id: input.get("_parent_turn_id").and_then(|v| v.as_u64()).map(|v| v as u32),
agent_label: input.get("prompt").and_then(|v| v.as_str()).map(|s| {
    let preview: String = s.chars().take(40).collect();
    if s.chars().count() > 40 { format!("{}…", preview) } else { preview }
}),
```

- [ ] **Step 3: Compile**

```bash
cargo check 2>&1 | grep "^error" | head -5
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(events): AgentTool propagates parent turn id and label"
```

---

### Task 4: CLI — track turn stack

**Files:**

- Modify: `ts-ipc/cli.ts`

- [ ] **Step 1: Find the stream/event handler**

```bash
grep -n "client.onNotification.'stream/event'" ts-ipc/cli.ts
```

Expected: line ~737.

- [ ] **Step 2: Add turn-stack state**

Right above `client.onNotification('stream/event', ...)` (around line 729-737), add:

```ts
type TurnInfo = {
  id: number;
  parent: number | null;
  label: string | null;
  start: number;
};
const turnStack: TurnInfo[] = [];
function turnDepth(): number {
  return turnStack.length;
}
function turnPrefix(): string {
  if (turnStack.length === 0) return "";
  return turnStack.map(() => "│ ").join("");
}
```

- [ ] **Step 3: Add cases for turn_start / turn_end**

Inside the `switch (event.type) { ... }`, add (before `default:`):

```ts
case 'turn_start': {
  const t = event as { turn_id: number; parent_turn_id: number | null; agent_label: string | null };
  turnStack.push({ id: t.turn_id, parent: t.parent_turn_id, label: t.agent_label, start: Date.now() });
  const labelText = t.agent_label ? ` ${FG_GRAY}${t.agent_label}${RESET}` : '';
  const depthBar = turnPrefix().slice(0, -2);
  const which = t.parent_turn_id != null ? `Subagent Turn ${t.turn_id}` : `Turn ${t.turn_id}`;
  console.log(`${depthBar}${FG_ORANGE}┌─ ${which}${labelText} ─${RESET}`);
  break;
}
case 'turn_end': {
  const t = event as { turn_id: number; duration_ms: number; tool_count: number; input_tokens: number; output_tokens: number };
  turnStack.pop();
  const depthBar = turnPrefix();
  const seconds = (t.duration_ms / 1000).toFixed(1);
  const totalTok = t.input_tokens + t.output_tokens;
  console.log(
    `${depthBar}${FG_ORANGE}└─ Turn ${t.turn_id} done${RESET} ${DIM}${t.tool_count} tools, ${seconds}s, ${formatTokens(totalTok)} tokens${RESET}`
  );
  break;
}
```

- [ ] **Step 4: Add formatTokens helper**

Right before the `client.onNotification('stream/event', ...)` block, add:

```ts
function formatTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
  return String(n);
}
```

- [ ] **Step 5: Compile-check the TS**

```bash
cd ts-ipc && npx tsc --noEmit cli.ts 2>&1 | head -10
```

Expected: no errors (warnings okay).

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(cli): render TurnStart/TurnEnd as nested boxes"
```

---

### Task 5: CLI — indent existing tool/text events by turn depth

**Files:**

- Modify: `ts-ipc/cli.ts`

- [ ] **Step 1: Add prefix to formatToolUse output**

Find the `case 'tool_use':` block (around line 763). Modify the `console.log(formatToolUse(...))` line:

```ts
console.log(turnPrefix() + formatToolUse(tu.tool_name, tu.input));
```

- [ ] **Step 2: Add prefix to formatToolResult output**

Find the `case 'tool_result':` block (around line 786). Modify the `console.log(formatToolResult(...))` line:

```ts
console.log(
  turnPrefix() +
    formatToolResult(tr.output, tr.is_error, toolInfo?.name, toolInfo?.input),
);
```

- [ ] **Step 3: Add prefix to assistant text flush**

Find where `process.stdout.write(`\\n${FG_ORANGE}${BOLD}BaoClaw${RESET}\\n`)` happens (around line 768-772):

```ts
process.stdout.write(`\n${turnPrefix()}${FG_ORANGE}${BOLD}BaoClaw${RESET}\n`);
const renderedLines = renderMarkdown(currentText).split("\n");
process.stdout.write(renderedLines.map((l) => turnPrefix() + l).join("\n"));
process.stdout.write("\n");
```

- [ ] **Step 4: Compile-check**

```bash
cd ts-ipc && npx tsc --noEmit cli.ts 2>&1 | head -10
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(cli): indent tool calls and text by turn depth"
```

---

### Task 6: Add /verbose command for log level toggling

**Files:**

- Modify: `ts-ipc/cli.ts`

- [ ] **Step 1: Add verbose state**

At the top of `main()`, near the other state variables (after `const turnStack: TurnInfo[] = [];`), add:

```ts
type LogLevel = "quiet" | "normal" | "verbose" | "debug";
let logLevel: LogLevel =
  (process.env.BAOCLAW_LOG_LEVEL as LogLevel) || "verbose";
```

- [ ] **Step 2: Add /verbose command handler**

Find where other slash commands are handled (search for `if (input === '/clear')`). Add a new branch:

```ts
if (input.startsWith("/verbose")) {
  const arg = input.slice("/verbose".length).trim();
  if (arg === "" || arg === "help") {
    console.log(`${DIM}Current log level: ${logLevel}${RESET}`);
    console.log(`${DIM}Levels: quiet | normal | verbose | debug${RESET}`);
    console.log(`${DIM}Usage: /verbose <level>${RESET}`);
  } else if (["quiet", "normal", "verbose", "debug"].includes(arg)) {
    logLevel = arg as LogLevel;
    console.log(`${FG_GREEN}✓ Log level set to ${logLevel}${RESET}`);
  } else {
    console.log(`${FG_RED}Unknown level: ${arg}${RESET}`);
  }
  rl.prompt();
  return;
}
```

- [ ] **Step 3: Use logLevel to filter event rendering**

In the `tool_result` case, replace:

```ts
console.log(
  turnPrefix() +
    formatToolResult(tr.output, tr.is_error, toolInfo?.name, toolInfo?.input),
);
```

with:

```ts
if (logLevel === "quiet") {
  // skip tool results in quiet mode
} else if (logLevel === "normal" && !tr.is_error) {
  // normal mode shows only error results, skip success
} else {
  console.log(
    turnPrefix() +
      formatToolResult(tr.output, tr.is_error, toolInfo?.name, toolInfo?.input),
  );
}
```

Apply similar logic to `thinking_chunk` (in 'quiet'/'normal' modes, skip thinking).

- [ ] **Step 4: Compile-check**

```bash
cd ts-ipc && npx tsc --noEmit cli.ts 2>&1 | head -10
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(cli): add /verbose command for log level filtering"
```

---

### Task 7: Status footer with token/cost meters

**Files:**

- Modify: `ts-ipc/cli.ts`

- [ ] **Step 1: Add session-cumulative state**

Near the other state vars in `main()`:

```ts
let cumulativeInputTokens = 0;
let cumulativeOutputTokens = 0;
let cumulativeCostUsd = 0;
const CONTEXT_WINDOW = 200_000; // could be read from server later
```

- [ ] **Step 2: Update on TurnEnd and Result events**

In the `case 'turn_end':` block, after `turnStack.pop();`, add:

```ts
cumulativeInputTokens += t.input_tokens;
cumulativeOutputTokens += t.output_tokens;
```

In the `case 'result':` block (find it via `grep`), after the existing code, add:

```ts
const r = event as { total_cost_usd?: number };
if (typeof r.total_cost_usd === "number") {
  cumulativeCostUsd = r.total_cost_usd;
}
```

- [ ] **Step 3: Print status line on result**

After updating `cumulativeCostUsd`, add a status line for that query result:

```ts
const usedTokens = cumulativeInputTokens + cumulativeOutputTokens;
const pct = ((cumulativeInputTokens / CONTEXT_WINDOW) * 100).toFixed(0);
const costStr = `$${cumulativeCostUsd.toFixed(4)}`;
console.log(
  `${DIM}┃ 🔤 ${formatTokens(cumulativeInputTokens)} / ${formatTokens(CONTEXT_WINDOW)} (${pct}%)   💰 ${costStr}   📊 ${formatTokens(usedTokens)} total${RESET}`,
);
```

- [ ] **Step 4: Compile-check**

```bash
cd ts-ipc && npx tsc --noEmit cli.ts 2>&1 | head -10
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(cli): status footer with token usage and cost"
```

---

### Task 8: Highlight special events (compact, fallback, retries)

**Files:**

- Modify: `ts-ipc/cli.ts`

- [ ] **Step 1: Find model_fallback case**

```bash
grep -n "model_fallback\|case 'model_fallback'" ts-ipc/cli.ts
```

Expected: a case already exists or needs to be added.

- [ ] **Step 2: Make model_fallback prominent**

Replace whatever the existing handler does (or add new) with:

```ts
case 'model_fallback': {
  const f = event as { from_model: string; to_model: string };
  console.log('');
  console.log(`${FG_YELLOW}🔀 Model fallback: ${f.from_model} → ${f.to_model}${RESET}`);
  console.log('');
  break;
}
```

- [ ] **Step 3: Detect compact-related progress events**

In the `case 'progress':` block, after determining `info`, add a special check:

```ts
const msg = String(pg.data?.message ?? "");
if (msg.includes("Compact") || msg.includes("compact")) {
  stopSpinner();
  console.log("");
  console.log(`${FG_CYAN}━━━━━ 📦 ${msg} ━━━━━${RESET}`);
  console.log("");
  return;
}
```

- [ ] **Step 4: Compile-check**

```bash
cd ts-ipc && npx tsc --noEmit cli.ts 2>&1 | head -10
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(cli): highlight model fallback and compaction events"
```

---

### Task 9: End-to-end visual test

**Files:** none (manual verification)

- [ ] **Step 1: Build everything**

```bash
cd baoclaw-core && cargo build --release && cd ..
cp baoclaw-core/target/release/baoclaw-core /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core.new
mv -f /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core.new /home/baohx@spdbfl/.baoclaw/bin/baoclaw-core
cp ts-ipc/cli.ts /home/baohx@spdbfl/.baoclaw/ts-ipc/cli.ts
```

- [ ] **Step 2: Restart daemon and test a sub-agent query**

Start a new session: `baoclaw`. Send: "Use the AgentTool to summarize the file query_engine.rs in 3 sentences."

Expected output (stylised):

```
┌─ Turn 1
│  🤖 agent  "Summarize the file query_engine.rs..."
│  ┌─ Subagent Turn 1  Summarize the file query_engine...
│  │  📄 read  baoclaw-core/src/engine/query_engine.rs
│  │  ✓ 2,951 lines
│  └─ Turn 1 done  1 tools, 2.4s, 12.3K tokens
│
│  BaoClaw
│  The query_engine.rs file...
└─ Turn 1 done  1 tools, 18.2s, 15.7K tokens
┃ 🔤 18.5K / 200K (9%)   💰 $0.0042   📊 19.1K total
```

- [ ] **Step 3: Verify /verbose works**

Run `/verbose normal` then send a query. Tool results should be hidden (only errors shown).

- [ ] **Step 4: Verify /verbose quiet**

Run `/verbose quiet` then send a query. Only the final assistant text should appear (no tools, no thinking).

- [ ] **Step 5: Final commit + tag**

```bash
git commit --allow-empty -m "feat(cli): turn-grouped agent visibility v1 complete"
git tag -a visibility-v1 -m "Turn-grouped CLI visibility shipped"
```

---

## Self-Review Notes

**Spec coverage:**

- ✅ Turn nesting → Tasks 1-3 (Rust events) + 4-5 (CLI render)
- ✅ Information density levels → Task 6 (`/verbose`)
- ✅ Token/cost dashboard → Task 7
- ✅ Special event highlights → Task 8
- ✅ Visual verification → Task 9

**No placeholders:** All TypeScript and Rust snippets are complete code, not pseudocode.

**Type consistency:** `TurnInfo`, `LogLevel`, `formatTokens()` defined once and reused.

**Note on test coverage:** This plan is light on automated tests (most tasks are CLI rendering). Task 9 is a manual visual verification. Stronger TDD would require a TUI test harness, which is out of scope for this plan.
