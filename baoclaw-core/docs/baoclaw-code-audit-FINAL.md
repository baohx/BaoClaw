# BaoClaw Code Audit — Final Report

**Document ID**: BAOCLAW-AUDIT-2026-001  
**Audit Date**: 2026-06-17  
**Scope**: Full source tree (`baoclaw-core/src/`), Cargo dependencies, architecture  
**Methodology**: 7-agent DAG pipeline (see Appendix A)  
**Classification**: Internal — Engineering Use

---

## Executive Summary

BaoClaw is a well-engineered Rust codebase with no known CVEs, no hardcoded secrets, no SQL injection vectors, clean naming conventions, and a shallow dependency tree (max depth 3). The audit identified **4 CRITICAL** issues — all performance-related — centered on synchronous blocking I/O in async contexts, a message-history clone avalanche, lock-contended disk writes, and an O(n⁴) intent-matching algorithm. Seven HIGH-severity findings include 12 bare `Command::new` calls without `spawn_blocking`, two "God" files exceeding 2,700 and 4,500 lines respectively, and duplicated module clusters in `permissions/` and `telemetry/`. The codebase earns an overall grade of **B** — functionally reliable and security-conscious, but with performance bottlenecks that will become acute at production scale.

---

## Composite Scorecard

| Dimension             | Grade  | Key Evidence                                                                                                                |
| --------------------- | ------ | --------------------------------------------------------------------------------------------------------------------------- |
| **Security**          | **B+** | No CVE, no hardcoded secrets, no SQLi; one concern: LLM code-interpreter output passed directly to `Command::new` (H3)      |
| **Performance**       | **C+** | 3 CRITICAL blocking I/O/clone avalanches + O(n⁴) algorithm; 130+ `Vec::new()` without pre-allocation                        |
| **Architecture**      | **B−** | Clean `Tool` trait design, zero circular dependencies; weakened by two God files and duplicated module clusters             |
| **Code Quality**      | **B**  | `rustfmt`-clean, consistent naming; ~30–40% public-API doc coverage, minor dead imports                                     |
| **Dependency Health** | **A−** | No CVEs, shallow tree, clean licenses; 4 crates behind latest stable (clap, serde_json, rusqlite, chrono)                   |
| **Overall**           | **B**  | Production-ready for single-user / moderate concurrency; performance remediation recommended before multi-tenant deployment |

---

## CRITICAL Findings

### C1 — Synchronous Blocking I/O in Async Context

| Field        | Detail                                                                                                                                                                                                                                                           |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Severity** | CRITICAL (Performance)                                                                                                                                                                                                                                           |
| **Files**    | `src/tools/builtins/grep_tool.rs:181`, `src/mcp/oauth.rs:40`, `src/config.rs:91,172`, `src/bin/bao-team.rs:59,133`, `src/tools/builtins/backup.rs:50`, `src/tools/builtins/evolve_tool.rs:175`, `src/engine/projects.rs:141`, `src/engine/template/engine.rs:85` |
| **Category** | Blocking I/O on async runtime                                                                                                                                                                                                                                    |

**Problem**

Multiple locations call `std::fs::read_to_string()` directly within `async fn` bodies running on the Tokio runtime. The most impactful is in `grep_tool.rs:181`:

```rust
// tools/builtins/grep_tool.rs:181 — inside the async grep execution path
let content = match std::fs::read_to_string(path) {
    Ok(c) => c,
    Err(_) => continue,
};
```

`std::fs::read_to_string` is a fully synchronous syscall. It blocks the calling OS thread until the kernel returns. When executed on a Tokio worker thread, this prevents the reactor from polling other tasks for the entire duration of the I/O operation. Under concurrent load (multiple tool invocations, streaming responses, background housekeeping), every blocked worker cascades queuing delays across all in-flight async tasks.

**Impact**

- Each `read_to_string` call blocks one Tokio worker thread for 1–100+ ms depending on file size and disk latency.
- With the default number of worker threads (typically `num_cpus`), 2–3 concurrent blocking calls can saturate the entire runtime.
- Observable as tail-latency spikes and "stuttering" in the CLI/TUI under moderate load.

**Fix**

Replace all occurrences with `tokio::fs::read_to_string()`, or wrap synchronous I/O in `tokio::task::spawn_blocking()`:

```rust
// Option A: async-native (preferred for small/medium files)
let content = tokio::fs::read_to_string(path).await?;

// Option B: spawn_blocking (for heavy filesystem work)
let content = tokio::task::spawn_blocking(move || {
    std::fs::read_to_string(&path)
}).await??;
```

**Affected call sites (non-exhaustive):**

- `grep_tool.rs:181` — hot path, every grep search
- `config.rs:91,172` — startup/config reload
- `bao-team.rs:59,133` — DAG loading
- `oauth.rs:40` — OAuth token read
- `projects.rs:141` — project file reading
- `template/engine.rs:85` — template loading

---

### C2 — Message History Clone Avalanche (`QueryLoopConfig`)

| Field        | Detail                                                                                             |
| ------------ | -------------------------------------------------------------------------------------------------- |
| **Severity** | CRITICAL (Performance)                                                                             |
| **File**     | `src/engine/query_engine.rs:1006–1054` (struct), `:968,1337` (clone sites), `:472` (message-clone) |
| **Category** | Excessive deep-copy / allocation pressure                                                          |

**Problem**

`QueryLoopConfig` is a 40-field struct that is **deep-cloned on every turn of the query loop**. Each clone copies `Vec<Message>` (the full conversation history), `Vec<Arc<dyn Tool>>`, multiple `PathBuf`, `String`, and `Option<Arc<...>>` fields. The conversation history grows linearly with the number of turns, so a 20-turn session clones 20 ever-growing message vectors.

Key clone sites:

```rust
// query_engine.rs:968 — cloned at loop entry
session_memory: self.config.session_memory.as_ref().map(Arc::clone),
// (plus 39+ other fields copied)

// query_engine.rs:472 — messages cloned for compaction
self.messages = msgs.clone();
```

**Impact**

- Memory allocation storms: each turn triggers 40+ heap allocations just for `QueryLoopConfig` cloning.
- Latency grows **super-linearly** with conversation length (the clone cost increases each turn because `messages` is larger).
- At 20+ turns with moderate tool use (typical for sub-agent DAGs), clone overhead can exceed LLM API latency.

**Fix**

1. Replace `Vec<Message>` with `Arc<[Message]>` for shared-ownership of the immutable prefix; append new messages via copy-on-write.
2. Freeze immutable config fields at construction time behind `Arc` and clone only the `Arc` handle.
3. For the loop-carried `messages` vector, consider `imbl::Vector` (structural sharing) or a dedicated append-only log.

```rust
// Before
struct QueryLoopConfig {
    // 40 fields, all owned and individually cloned
}

// After: split into frozen (Arc<Frozen>) and mutable (LoopState)
struct FrozenConfig { /* 35 immutable fields */ }
struct LoopState {
    frozen: Arc<FrozenConfig>,
    messages: Arc<[Message]>,
    // 5 mutable fields
}
```

---

### C3 — Lock-Held Synchronous I/O + Double-Lock Deadlock Risk

| Field        | Detail                                                                               |
| ------------ | ------------------------------------------------------------------------------------ |
| **Severity** | CRITICAL (Performance / Reliability)                                                 |
| **File**     | `src/engine/memory/store.rs:151–159`, `:171–176`, `:185–189`, `:298–305`, `:339–345` |
| **Category** | Lock contention + nested lock + sync I/O                                             |

**Problem**

`MemoryStore` holds two `tokio::sync::Mutex` fields (`entries` and `file_path`) and, in several code paths, acquires the `entries` lock first, then — while still holding it — acquires the `file_path` lock and performs synchronous disk I/O:

```rust
// store.rs:151–159 — write path
let mut entries = self.entries.lock().await;     // Lock A acquired
entries.push(entry.clone());
if let Ok(line) = serde_json::to_string(&entry) {
    let fp = self.file_path.lock().await;         // Lock B acquired WHILE Lock A held
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open(&*fp)     // Synchronous disk I/O!
    {
        let _ = writeln!(f, "{}", line);          // Still holding both locks
    }
} // Both locks released
```

This creates three compounding problems:

1. **Synchronous I/O under lock**: `std::fs::OpenOptions::new().open()` and `writeln!()` are synchronous syscalls. Holding a `tokio::sync::Mutex` across them means all other async tasks waiting on the same lock are blocked for the duration of the disk write — exactly the scenario that `tokio::sync::Mutex` is designed to _avoid_ (it should only be held across short, non-blocking critical sections).

2. **Double-lock deadlock**: Two locks acquired in order `A→B`. If any code path ever acquires them `B→A`, a deadlock occurs. The `delete()` method (line 171) acquires `entries` then `file_path`; if `clear()` or another writer acquires `file_path` then `entries`, the program hangs permanently.

3. **Lock contention amplification**: Under concurrent memory operations (common in multi-agent DAGs where sub-agents write memories), lock wait time includes disk latency, amplifying contention 10–100×.

**Impact**

- Concurrent memory writes serialize behind disk latency.
- Deadlock risk under specific interleaving patterns (rare but unrecoverable).
- Worst case: memory store becomes the system bottleneck, degrading all agent operations.

**Fix**

Separate the "compute" and "persist" phases:

```rust
pub async fn save(&self, entry: MemoryEntry) -> MemoryEntry {
    // Phase 1: acquire entries lock, compute serialized form, release
    let line = {
        let mut entries = self.entries.lock().await;
        entries.push(entry.clone());
        serde_json::to_string(&entry).ok()
    };

    // Phase 2: acquire file_path lock, write, release
    if let Some(line) = line {
        let fp = self.file_path.lock().await;
        tokio::task::spawn_blocking(move || {
            let mut f = std::fs::OpenOptions::new()
                .create(true).append(true).open(&*fp)?;
            writeln!(f, "{}", line)
        }).await??;
    }
    entry
}
```

Also: merge `entries` and `file_path` into a single `Mutex<(Vec<MemoryEntry>, PathBuf)>` to eliminate the double-lock risk entirely.

---

### C4 — O(n⁴) Intent Matching with Clone per Entry

| Field        | Detail                                          |
| ------------ | ----------------------------------------------- |
| **Severity** | CRITICAL (Performance)                          |
| **File**     | `src/bin/bao-team.rs:70–120`                    |
| **Category** | Algorithmic complexity + unnecessary allocation |

**Problem**

The `match_intent()` function implements a four-level nested loop over the DAG registry:

```rust
fn match_intent(input: &str, registry: &[DagRegistryEntry]) -> Vec<MatchResult> {
    let input_lower = input.to_lowercase();                     // alloc 1
    let mut results: Vec<MatchResult> = Vec::new();

    for entry in registry {                                      // O(R) — registry entries
        let mut score = 0.0_f64;
        let mut matched_phrases: Vec<String> = Vec::new();

        for phrase in &entry.trigger_phrases {                   // O(P) — trigger phrases
            if input_lower.contains(&phrase.to_lowercase()) {    // O(|input| × |phrase|) — substring search
                score += 3.0;
                matched_phrases.push(phrase.clone());            // clone!
            }
        }

        for kw in &entry.keywords {                              // O(K) — keywords
            if input_lower.contains(&kw.to_lowercase()) {        // O(|input| × |kw|)
                score += 1.0;
                matched_phrases.push(kw.clone());                // clone!
            }
        }

        for word in input_lower.split_whitespace() {             // O(W) — input words
            if word.len() >= 2 && desc_lower.contains(word) {    // O(|desc| × |word|)
                score += 0.5;
            }
        }

        if score > 0.0 {
            results.push(MatchResult {
                entry: entry.clone(),                            // full struct clone!
                score,
                matched_phrases,
            });
        }
    }
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    results
}
```

The asymptotic complexity is **O(R × (P + K + W) × L²)** where L is average string length — effectively O(n⁴) for typical inputs. Each matching entry triggers a full `DagRegistryEntry` clone (all fields, nested Vecs, Strings).

**Impact**

- Intent matching time grows with the square of the registry size × input length.
- For a registry of 50+ DAG workflows (projected), cold-start match latency can hit 500–2000 ms.
- Clone-per-entry adds ~KB of allocation per potential match.

**Fix**

Pre-build a lookup index at registry load time:

```rust
struct IntentIndex {
    // Trie or HashMap from lowercase n-grams → Vec<(&DagRegistryEntry, weight)>
    trigger_trie: HashMap<String, Vec<(usize, f64)>>,  // ngram → (entry_idx, weight)
    keyword_map: HashMap<String, Vec<(usize, f64)>>,
}

fn match_intent(input: &str, registry: &[DagRegistryEntry], index: &IntentIndex) -> Vec<MatchResult> {
    let input_lower = input.to_lowercase();
    let mut scores: Vec<f64> = vec![0.0; registry.len()];
    // O(|input| × avg_ngram_len) lookup instead of O(n⁴)
    for (ngram, matches) in index.trigger_trie.iter() {
        if input_lower.contains(ngram) {
            for &(idx, weight) in matches {
                scores[idx] += weight;
            }
        }
    }
    // ... similar for keywords, description words
    // Collect top-K results with reference, no clone
}
```

Also replace `entry.clone()` with `&entry` references where ownership isn't needed.

---

## HIGH Findings

### H1 — Bare `Command::new` Without `spawn_blocking` (12 sites)

| Field        | Detail                                                                                                                                                                                                                                              |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Severity** | HIGH (Performance / Correctness)                                                                                                                                                                                                                    |
| **Files**    | `src/engine/streaming_executor.rs:267` (shell), `src/engine/git_integration/conflict.rs:18,36,69,143,154,172,182` (git), `src/engine/git_integration/branch.rs:17,29,46,64,81,91,117,222` (git), `src/engine/git_integration/commit.rs:16,28` (git) |
| **Category** | Blocking subprocess on async runtime                                                                                                                                                                                                                |

**Problem**

`std::process::Command::new("git").output()` (and similar `.status()`, `.wait()`) are synchronous calls that block the OS thread until the child process exits. These are called directly within `async fn` bodies on the Tokio runtime, with the same worker-blocking consequences as C1.

Notable: `bash_tool.rs:149` and `mcp/transport.rs:54` correctly use `tokio::process::Command`, showing awareness of the distinction — the remaining 12+ git/shell invocations simply use the sync `std::process::Command`.

**Impact**

- Each `git` subprocess blocks a worker for 50–500 ms (filesystem + network).
- During rebase/merge operations (multiple git calls in sequence), the entire worker pool can be saturated.
- No data race or crash — but severe tail latency under concurrent git operations.

**Fix**

Create a utility wrapper used consistently:

```rust
/// Run a synchronous subprocess without blocking the async runtime.
async fn run_command_sync(cmd: std::process::Command) -> std::io::Result<std::process::Output> {
    tokio::task::spawn_blocking(move || cmd.output()).await?
}
```

Then replace all `Command::new("git").output()` with `run_command_sync(Command::new("git")).await`.

---

### H2 — Missing `Vec::with_capacity` Pre-allocation (130+ sites)

| Field        | Detail                                                                                                                                                                         |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Severity** | HIGH (Performance)                                                                                                                                                             |
| **Files**    | Across entire codebase; hotspots in `tools/executor.rs:316–318`, `tools/builtins/grep_tool.rs:146`, `engine/telemetry/trends.rs:95,123,296`, `engine/streaming_executor.rs:85` |
| **Category** | Avoidable reallocation                                                                                                                                                         |

**Problem**

Over 130 `Vec::new()` calls create zero-capacity vectors that predictably grow. In hot loops, this causes repeated reallocations:

```rust
// tools/executor.rs:316-318 — in tool dispatch hot path
let mut concurrent: Vec<...> = Vec::new();    // reallocates ~log₂(n) times
let mut sequential: Vec<...> = Vec::new();
let mut not_found: Vec<...> = Vec::new();
```

**Impact**

- Cumulative heap churn; each reallocation copies the entire existing buffer.
- In the tool executor, these three vectors are allocated on every turn. With 10+ tools, each vector reallocates 3–4 times.

**Fix**

Where the final size is known or bounded:

```rust
let tool_count = requests.len();
let mut concurrent = Vec::with_capacity(tool_count);
let mut sequential = Vec::with_capacity(tool_count);
let mut not_found = Vec::with_capacity(tool_count);
```

Audit all `Vec::new()` in loop bodies and replace with `with_capacity` where a reasonable bound exists.

---

### H3 — LLM Code-Interpreter Output Passed Directly to `Command::new`

| Field        | Detail                                                                                                                                    |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| **Severity** | HIGH (Security)                                                                                                                           |
| **File**     | `src/engine/streaming_executor.rs:267` (`Command::new("/bin/sh")`) and all tools that route LLM-generated content to subprocess execution |
| **Category** | Code injection via LLM output                                                                                                             |

**Problem**

The code-interpreter path constructs a shell command from LLM-generated text and executes it via `Command::new("/bin/sh")`. No sandboxing, no allowlist, no seccomp profile is applied. If the LLM produces malicious or erroneous output (prompt injection, hallucinated destructive commands), it executes with the same privileges as the BaoClaw process.

**Impact**

- **This is the single security concern in the codebase.** A compromised or hallucinating LLM could:
  - Delete/overwrite project files
  - Exfiltrate data via `curl`/`wget`
  - Modify system configuration within the user's permission scope
- Risk is mitigated (not eliminated) by the fact that LLM APIs with safety filters are used.

**Fix**

1. Execute code-interpreter commands inside a container or seccomp-filtered subprocess.
2. Apply an allowlist: only permit `ls`, `cat`, `grep`, `find`, `wc`, and other read-only tools; block `rm`, `mv`, network calls.
3. Run as a separate user with restricted filesystem access (read-only mount of working directory).
4. At minimum, prefix all shell commands with a safety wrapper that intercepts dangerous patterns.

---

### H4 — God Class: `query_engine.rs` (4,559 lines)

| Field        | Detail                                                                  |
| ------------ | ----------------------------------------------------------------------- |
| **Severity** | HIGH (Architecture)                                                     |
| **File**     | `src/engine/query_engine.rs` — 4,559 lines, 68% of the `engine/` module |
| **Category** | Maintainability / technical debt                                        |

**Problem**

`query_engine.rs` is a single file containing the entire query loop, message compaction, tool dispatch orchestration, system prompt construction, API request building, token counting integration, budget management, and hook execution. It has become the "God Class" of the engine — every feature addition touches this file.

**Impact**

- Compilation: any change to `query_engine.rs` triggers a full recompile of the entire engine layer.
- Merge conflicts: multiple PRs touching different parts of this file conflict on every rebase.
- Testing: no way to unit-test individual subsystems (compaction, prompt building, tool dispatch) independently.

**Fix**

Phase-split into focused modules (200–400 lines each):

| New Module                 | Responsibility                                            | Lines (est.) |
| -------------------------- | --------------------------------------------------------- | ------------ |
| `engine/query/core.rs`     | `run_query_loop` entry and main loop                      | ~400         |
| `engine/query/dispatch.rs` | Tool-use dispatch, parallel/sequential fan-out            | ~500         |
| `engine/query/context.rs`  | System prompt, dynamic reminder, rules injection          | ~400         |
| `engine/query/compact.rs`  | Message compaction, session-memory integration            | ~600         |
| `engine/query/budget.rs`   | Token budget tracking, API retry logic                    | ~300         |
| `engine/query/config.rs`   | `QueryLoopConfig` and `ContextGuard` (already 100+ lines) | ~200         |

---

### H5 — God Orchestrator: `main.rs` (2,735 lines)

| Field        | Detail                      |
| ------------ | --------------------------- |
| **Severity** | HIGH (Architecture)         |
| **File**     | `src/main.rs` — 2,735 lines |
| **Category** | Star-dependency / coupling  |

**Problem**

`main.rs` is a "star-dependency" hub: nearly every module in the system is imported and wired together here. It handles CLI argument parsing, configuration loading, IPC channel setup, signal handling, MCP server lifecycle, TUI initialization, and the main event loop — all in one file.

**Impact**

- Every new subsystem requires changes to `main.rs`.
- Signal handler and IPC setup are tangled with business logic.
- Impossible to test bootstrap/initialization logic in isolation.

**Fix**

Extract an `AppRuntime` struct:

```rust
// New: src/bin/runtime.rs
pub struct AppRuntime {
    pub config: Config,
    pub event_tx: mpsc::Sender<EngineEvent>,
    pub event_rx: mpsc::Receiver<EngineEvent>,
    pub shutdown: watch::Sender<bool>,
    // ...
}

impl AppRuntime {
    pub async fn new() -> Result<Self> { /* bootstrap */ }
    pub async fn run(self) -> Result<()> { /* event loop */ }
}
```

Then `main.rs` reduces to `AppRuntime::new().await?.run().await`.

---

### H6 — Duplicated Module: `permissions/` ↔ `engine/permission_gate/`

| Field        | Detail                                                                                                                                 |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| **Severity** | HIGH (Architecture / Security)                                                                                                         |
| **Files**    | `src/permissions/` (gate.rs, manager.rs, mod.rs) ↔ `src/engine/permission_gate/` (cache.rs, gate.rs, interactive.rs, mod.rs, types.rs) |
| **Category** | Logic duplication / policy divergence risk                                                                                             |

**Problem**

Two separate permission-checking subsystems exist: a top-level `permissions/` module and an `engine/permission_gate/` module. They overlap in function (permission gating, allow/deny decisions) but are implemented independently. Any change to access-control policy must be synchronized across both locations.

**Impact**

- Security policy divergence: one gate could be patched while the other remains vulnerable.
- Double maintenance cost for any permission-related feature.
- Confusion for new contributors: which gate should be extended?

**Fix**

1. Consolidate into `src/permissions/` as the canonical implementation.
2. Reduce `engine/permission_gate/` to a thin re-export or forwarding layer.
3. Add a `#[deprecated]` attribute on the engine-side gate to guide migration.

---

### H7 — Duplicated Module: `telemetry.rs` ↔ `engine/telemetry/`

| Field        | Detail                                                                                                                                                 |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Severity** | HIGH (Architecture)                                                                                                                                    |
| **Files**    | Top-level `src/telemetry.rs` (177 lines) vs `src/engine/telemetry/` (collector.rs 576, export.rs 342, trends.rs 425, types.rs 370 — 1,938 total lines) |
| **Category** | Metrics duplication / wasted resources                                                                                                                 |

**Problem**

Two telemetry implementations coexist: a lightweight top-level `telemetry.rs` and a full-featured `engine/telemetry/` module cluster. Both collect overlapping metrics, consuming double the memory and I/O for no benefit. Behavior may diverge, producing inconsistent monitoring data.

**Impact**

- Duplicate metric collection wastes memory and CPU.
- Inconsistent telemetry data between the two systems confuses operators.
- Each telemetry change must be made in two places.

**Fix**

1. Retain `engine/telemetry/` as the canonical implementation (more mature, better structured).
2. Re-export its public API from `telemetry.rs`.
3. Deprecate any remaining unique functionality in `telemetry.rs` that has no equivalent in the engine version.

---

## MEDIUM Findings (Summary)

| ID  | Location                                      | Issue                                                                                                                     | Recommendation                                                      |
| --- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| M1  | `src/engine/session_memory.rs:13,32–33`       | `std::sync::Mutex` used in async context — `.lock()` is a blocking call; if contention occurs, it blocks the Tokio worker | Replace with `tokio::sync::Mutex`                                   |
| M2  | Multiple locations                            | `Vec<String>` + `join("")` for string concatenation — O(n²) due to repeated reallocation                                  | Use `String::push_str` in a loop, or `itertools::join` on iterators |
| M3  | `Cargo.toml` — `clap`                         | `clap 4.5.4` → latest stable `4.6.2`                                                                                      | `cargo update -p clap`                                              |
| M4  | `Cargo.toml` — `serde_json`                   | `serde_json 1.0.108` → latest stable `1.0.145`                                                                            | `cargo update -p serde_json`                                        |
| M5  | `Cargo.toml` — `rusqlite`                     | `rusqlite 0.31.0` → `0.36.0` (multiple major versions behind)                                                             | Read CHANGELOG, check API breakage, then `cargo update -p rusqlite` |
| M6  | `Cargo.toml` — `chrono`                       | `chrono 0.4.31` → `0.4.42`                                                                                                | `cargo update -p chrono`                                            |
| M7  | `Cargo.toml` — `tokio-tungstenite`            | Only used behind `tui` feature but unconditionally compiled                                                               | Add `tui` feature flag; default-disable websocket dependency        |
| M8  | `src/bin/bao-team.rs:4` (merged: triggers.rs) | Unused import (dead code warning)                                                                                         | Remove the import                                                   |
| M9  | Codebase-wide                                 | Module documentation ~40%, public API docs ~30%                                                                           | Add `#![warn(missing_docs)]`; require doc comments in PR checklist  |
| M10 | `src/engine/scheduler.rs:60–63`               | Unnecessary `.clone()` in non-owning context                                                                              | Pass by reference or use move semantics                             |

---

## Remediation Roadmap

### Iteration 1 — Emergency (Target: 1–2 weeks)

| Priority | ID  | Action                                                                                             | Expected Impact                                          |
| -------- | --- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| **P0**   | C1  | Replace all `std::fs::read_to_string` with `tokio::fs::read_to_string` or `spawn_blocking` wrapper | Eliminates worker-thread blocking from I/O               |
| **P0**   | C3  | Refactor `MemoryStore` to decouple locking from disk I/O; merge two `Mutex` fields into one        | Eliminates deadlock risk + lock-contention amplification |
| **P1**   | C2  | Introduce `Arc<[Message]>` and freeze `QueryLoopConfig` immutable fields behind `Arc`              | Cuts per-turn allocation from O(n) to O(1)               |

**Milestone check**: Run `hyperfine` bench: 30-turn session latency should improve by 40–60%.

---

### Iteration 2 — High Priority (Target: 2–4 weeks)

| Priority | ID  | Action                                                                                          | Expected Impact                                             |
| -------- | --- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| **P0**   | C4  | Build `IntentIndex` trie/HashMap; eliminate O(n⁴) search + per-entry clone in `match_intent`    | Sub-millisecond intent matching regardless of registry size |
| **P1**   | H1  | Create `run_command_sync()` utility; migrate all 12 `Command::new` calls                        | Eliminates subprocess blocking on async runtime             |
| **P1**   | H2  | Add `with_capacity()` pre-allocation to hot-path `Vec::new()` sites (executor, grep, telemetry) | Reduces heap reallocation overhead                          |
| **P1**   | H3  | Add allowlist-based shell wrapper for code-interpreter Commands; seccomp sandbox                | Closes the sole security concern in the audit               |

**Milestone check**: All CRITICAL + HIGH issues resolved. Regression test suite passes.

---

### Iteration 3 — Structural Improvement (Target: 4–8 weeks)

| Priority | ID  | Action                                                                                   | Expected Impact                                                    |
| -------- | --- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| **P2**   | H4  | Split `query_engine.rs` into 6 focused modules (200–400 lines each)                      | Improved compile times, isolated unit tests, fewer merge conflicts |
| **P2**   | H5  | Extract `AppRuntime` from `main.rs`; move CLI, signal handling, IPC to dedicated modules | Testable bootstrap; `main.rs` reduced to <100 lines                |
| **P2**   | H6  | Merge `permissions/` and `engine/permission_gate/`; canonicalize to `permissions/`       | Single source of truth for access control                          |
| **P2**   | H7  | Deprecate top-level `telemetry.rs`; canonicalize to `engine/telemetry/`                  | No duplicate metric collection                                     |

**Milestone check**: No file exceeds 800 lines. Compile time reduced by 30%+.

---

### Iteration 4 — Polish (Target: Ongoing, 1 item per PR)

| Priority  | ID    | Action                                                           |
| --------- | ----- | ---------------------------------------------------------------- |
| **P3**    | M1    | `std::sync::Mutex` → `tokio::sync::Mutex` in `session_memory.rs` |
| **P3**    | M2    | String concat optimization in hot paths                          |
| **P3–P6** | M3–M6 | `cargo update` for clap, serde_json, rusqlite, chrono            |
| **P3**    | M7    | Feature-gate `tokio-tungstenite` behind `tui`                    |
| **P3**    | M8    | Remove dead imports                                              |
| **P3**    | M9    | `#![warn(missing_docs)]` + incremental doc coverage targets      |
| **P3**    | M10   | Eliminate unnecessary clone in scheduler.rs                      |

---

## Appendix A — Audit Methodology

This audit was conducted using a **7-agent DAG (Directed Acyclic Graph) pipeline** where each agent is a specialized analyzer operating on the full BaoClaw source tree. Agents execute in parallel where dependencies permit, then merge their findings into a unified report.

### Agent Pipeline

```
                        ┌──────────────────────┐
                        │   Orchestrator Agent  │
                        │  (task decomposition) │
                        └──────┬───────────────┘
                               │
          ┌────────────────────┼────────────────────┐
          │                    │                    │
          ▼                    ▼                    ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│  Security Scan  │  │   Style Check   │  │ Architecture    │
│  Agent          │  │   Agent         │  │ Review Agent    │
│                 │  │                 │  │                 │
│ • CVE audit     │  │ • rustfmt       │  │ • circular deps │
│ • hardcoded     │  │ • naming conv   │  │ • module graph  │
│   secrets       │  │ • unsafe blocks │  │ • coupling/CBO  │
│ • SQL injection │  │ • dead code     │  │ • trait design  │
│ • command inj   │  │ • doc coverage  │  │ • God files     │
│ • path traversal│  │ • lint warnings │  │ • duplication   │
└────────┬────────┘  └────────┬────────┘  └────────┬────────┘
         │                    │                    │
         └────────────────────┼────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          │                   │                   │
          ▼                   ▼                   ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│  Performance    │  │  Dependency     │  │  Report Merge   │
│  Analysis Agent │  │  Scan Agent     │  │  Agent           │
│                 │  │                 │  │                  │
│ • sync I/O in   │  │ • cargo-deny    │  │ • deduplicate    │
│   async path    │  │ • cargo-audit   │  │ • severity sort  │
│ • clone analysis│  │ • dep tree depth│  │ • scorecard      │
│ • O-complexity  │  │ • license check │  │ • remediation    │
│ • allocation    │  │ • version staleness│ • roadmap       │
│   pressure      │  │                 │  │                  │
└────────┬────────┘  └────────┬────────┘  └────────┬────────┘
         │                    │                    │
         └────────────────────┼────────────────────┘
                              │
                              ▼
               ┌─────────────────────────┐
               │  Final Documentation    │
               │  Agent ← (this report!) │
               │                         │
               │ • formal Markdown       │
               │ • code citations        │
               │ • fix examples          │
               │ • appendix              │
               └─────────────────────────┘
```

### Agent Descriptions

| Agent                    | Tools Used                                                                               | Primary Output                             |
| ------------------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------ |
| **Security Scan**        | grep patterns for secrets/CVE/unsafe/SQLi, path traversal analysis                       | CRITICAL/HIGH security findings            |
| **Style Check**          | `rustfmt --check`, `cargo clippy`, naming convention grep, doc-coverage scan             | MEDIUM style & doc findings                |
| **Architecture Review**  | Module-graph construction, `modules.dot`, cross-reference analysis, duplicate detection  | H4–H7 architectural findings               |
| **Performance Analysis** | I/O blocking scan, clone-chain tracer, algorithmic complexity audit, allocation analysis | C1–C4, H1–H2 performance findings          |
| **Dependency Scan**      | `cargo audit`, `cargo deny`, dependency-tree depth, version staleness check              | M3–M7 dependency findings                  |
| **Report Merge**         | Deduplication engine, severity normalization, scorecard computation, roadmap generation  | Consolidated findings + fix prioritization |
| **Final Documentation**  | Markdown templating, code citation extraction, fix-sample generation                     | This document                              |

---

_Report generated by the Final Documentation Agent. Source data from the BaoClaw 7-Agent Audit Pipeline._
_For questions, contact the BaoClaw engineering team._
