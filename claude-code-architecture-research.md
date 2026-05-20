# Claude Code Architecture & Design: Comprehensive Technical Research

> Based on analysis of leaked source code (v2.1.88, ~4,600 files, ~512K lines TypeScript),
> official Anthropic documentation, academic papers (arXiv:2604.14228), and community reverse-engineering.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Agent Loop / Harness Architecture](#2-agent-loop--harness-architecture)
3. [Memory Management System](#3-memory-management-system)
4. [Tool System Design](#4-tool-system-design)
5. [Context Window Management & Compaction](#5-context-window-management--compaction)
6. [Sub-Agent Architecture](#6-sub-agent-architecture)
7. [Permission & Security Model](#7-permission--security-model)
8. [Self-Improvement & Learning Mechanisms](#8-self-improvement--learning-mechanisms)
9. [Architecture Diagrams](#9-architecture-diagrams)
10. [Key Constants & Metrics](#10-key-constants--metrics)

---

## 1. Executive Summary

**Core Insight**: Only **1.6%** of Claude Code's codebase is AI decision logic. The other **98.4%** is deterministic infrastructure — permission gates, context management, tool routing, and recovery logic. The agent loop is a simple `while(true)`; the real engineering complexity lives in the systems *around* it.

**Tech Stack**: Bun runtime, TypeScript (strict mode), React 18 + Ink (terminal UI), Anthropic SDK, MCP SDK, GrowthBook (feature flags), Bun bundler with `feature()` dead-code elimination.

**Scale**: ~1,884 source files, ~512K lines of code, 54 tools, 27 hook events, 4 extension mechanisms, 7 permission modes, 7 safety layers, 5 compaction stages.

**Design Philosophy**: 5 human values → 13 design principles → implementation:
- **Human Decision Authority** — humans retain control via principal hierarchy
- **Safety, Security, Privacy** — protects even when human vigilance lapses
- **Reliable Execution** — gather-act-verify loop, graceful recovery
- **Capability Amplification** — "A Unix utility, not a product"
- **Contextual Adaptability** — CLAUDE.md hierarchy, graduated extensibility

---

## 2. Agent Loop / Harness Architecture

### 2.1 The Core: `query.ts` — A 1,729-line `while(true)` Loop

The heart of Claude Code is `query.ts`, implemented as an **async generator**:

```typescript
export async function* query(
  params: QueryParams,
): AsyncGenerator<
  | StreamEvent
  | RequestStartEvent
  | Message
  | TombstoneMessage
  | ToolUseSummaryMessage,
  Terminal  // Return value: termination reason
>
```

**Why async generator?** It unifies three channels into one function:
1. **Streaming events** — via `yield`
2. **Normal termination** — via `return` (Terminal type)
3. **Error propagation** — via natural `throw`

This avoids the scattered callback/event-emitter pattern where "received events but missed the termination signal" bugs become common.

### 2.2 State Management: Immutable Parameters + Mutable State

**Immutable parameters** (don't change throughout the loop):
```typescript
type QueryParams = {
  messages: Message[]
  systemPrompt: SystemPrompt
  canUseTool: CanUseToolFn       // Permission check callback
  toolUseContext: ToolUseContext   // Tool execution context
  taskBudget?: { total: number }  // API task_budget
  maxTurns?: number               // Maximum turn limit
  fallbackModel?: string          // Fallback model
  querySource: QuerySource        // Query source (REPL, agent, etc.)
}
```

**Mutable state** (updated each iteration via "Continue Site" pattern):
```typescript
type State = {
  messages: Message[]
  toolUseContext: ToolUseContext
  autoCompactTracking: AutoCompactTrackingState | undefined
  maxOutputTokensRecoveryCount: number
  hasAttemptedReactiveCompact: boolean
  maxOutputTokensOverride: number | undefined
  pendingToolUseSummary: Promise<ToolUseSummaryMessage | null> | undefined
  stopHookActive: boolean | undefined
  turnCount: number
  transition: Continue | undefined  // Continuation reason from previous iteration
}
```

**Continue Site Pattern**: Instead of modifying 9 fields individually, the entire state object is reassigned atomically:
```typescript
state = {
  ...state,
  messages: newMessages,
  turnCount: nextTurnCount,
  transition: { reason: 'next_turn' }
}
```
This prevents half-updated states and makes state transitions testable.

### 2.3 The 6-Stage Per-Turn Pipeline

Every turn goes through these stages:

#### Stage 1: Pre-Request Compaction (lines 365-548)
Five compaction mechanisms applied cheapest-first:
1. Tool Result Budget — caps oversized tool results
2. Snip Compact — drops old messages wholesale (cheapest, most aggressive)
3. Microcompact — selectively clears individual tool results, cache-aware
4. Context Collapse — reduces message blocks in stages (non-destructive)
5. Auto-Compact — summarizes entire conversation via LLM call (most expensive)

#### Stage 2: API Call & Streaming (lines 659-863)
- **StreamingToolExecutor**: begins executing tools as they stream in (latency optimization) — files are already being read while Claude is typing "Let me read that file"
- **Model fallback**: automatically switches to pre-designated fallback model on failure
- **Tombstone messages**: when model switches mid-stream, orphaned tool calls get tombstone markers (borrowing the database concept) to maintain history consistency

#### Stage 3: Error Recovery Cascade (lines 1062-1256)
**Prompt-too-long (413 error) — 3-stage cascade:**
1. Context Collapse drain (cost: 0) — commit pre-prepared reduction candidates
2. Reactive Compact (cost: 1 API call) — full summarization with strip-retry fallback
3. Surface the error to user

**Max-output-tokens recovery — 3 stages:**
1. Token cap escalation (cost: 0) — transparently increase 8K → 64K
2. Resume message injection (up to 3 times) — "Your previous response was truncated, continue"
3. Recovery exhaustion — complete with whatever results available

#### Stage 4: Stop Hooks & Token Budget (lines 1267-1355)
- Stop hooks execute user-defined validation logic
- **Diminishing returns detection**: if Claude continues 3+ times consecutively but produces <500 tokens each time, the system decides "continuing further is pointless" and stops
- Budget keeps going until 90% consumed, but won't spin wheels

#### Stage 5: Tool Execution (lines 1363-1520)
- Streaming results collected from concurrency-safe tools
- Remaining tools executed sequentially
- Real-time progress signals to UI

#### Stage 6: Post-Tool & Next Turn Transition (lines 1547-1727)
- **Prefetch pattern**: slow operations (fetch skill list, load memory) kicked off in Stage 1; results harvested here
- Queued commands drained
- MCP server tools refreshed
- State transitions and loop back

### 2.4 9 Termination Reasons

| Exit Reason | Meaning | Trigger |
|---|---|---|
| `completed` | Normal completion (no tool calls) | line 1264, 1357 |
| `blocking_limit` | Hard token limit reached | line 646 |
| `aborted_streaming` | User Ctrl+C during streaming | line 1051 |
| `aborted_tools` | User Ctrl+C during tool execution | line 1515 |
| `prompt_too_long` | Exceeded limit after recovery | line 1175, 1182 |
| `image_error` | Image validation failure | line 977, 1175 |
| `model_error` | Unexpected model error | line 996 |
| `hook_stopped` | Stop hook blocked continuation | line 1520 |
| `max_turns` | Maximum turn count exceeded | line 1711 |

### 2.5 QueryEngine.ts — The Session-Level Supervisor

`QueryEngine.ts` (1,295 lines) manages the entire session across multiple user inputs:

```typescript
class QueryEngine {
  mutableMessages: Message[]       // Full conversation history
  permissionDenials: PermissionDenial[]  // Tool permission denial records
  totalUsage: Usage                // Cumulative token usage
  readFileState: FileStateCache    // File state cache (prevents duplicate reads)
  discoveredSkillNames: Set<string> // Discovered skills (reset per turn)
  loadedNestedMemoryPaths: Set<string> // Loaded memory paths (prevents duplicates)
}
```

**Asymmetric transcript recording:**
- User messages: `await recordTranscript(userMessage)` — blocking save (essential for `--resume`)
- Assistant messages: `recordTranscript(assistantMessage)` — fire-and-forget (not critical for session restoration)

This cuts disk I/O wait time roughly in half.

### 2.6 Startup Sequence

**Phase 1: Fast-Path Routing** (cli.tsx — zero imports for --version)
- Trivial commands handled with zero module imports
- Default interactive path loads heavy main.tsx

**Phase 2: Initialization** (main.tsx + init.ts, memoized)
- Load settings, configure proxy/TLS, preconnect Anthropic API
- Fire-and-forget: event logging, OAuth, repository detection, remote settings

**Phase 3: Telemetry + Permissions**
- OpenTelemetry setup, permission context, trust dialog

**Phase 4: Setup** (parallel with command loading)
- CWD, hooks, worktree, file watchers, session memory

**Phase 5: Command & Agent Loading** (parallel with setup)
- `Promise.all([getCommands(cwd), getAgentDefinitionsWithOverrides(cwd)])`

**Phase 6: REPL Launch**
- Dynamic imports of React components, Ink TUI starts

---

## 3. Memory Management System

### 3.1 4-Level CLAUDE.md Hierarchy

Claude Code uses a file-based memory system (no vector DB) — fully inspectable, editable, version-controllable:

| Level | Path | Scope |
|---|---|---|
| **Managed** | `/etc/CLAUDE.md` | Organization-wide, admin-managed |
| **User** | `~/.claude/CLAUDE.md` | Personal preferences, cross-project |
| **Project** | `CLAUDE.md` + `.claude/rules/*.md` | Shared project conventions |
| **Local** | `CLAUDE.local.md` (gitignored) | Personal project-specific notes |

**Critical detail**: CLAUDE.md instructions are delivered as **user context** (probabilistic compliance), NOT system prompt (deterministic). This means the model *may* choose to override them.

### 3.2 Auto-Memory (Session Memory)

Claude automatically accumulates knowledge across sessions without user writing anything. Saved notes include:
- Build commands discovered
- Debugging insights
- Architecture notes
- Code patterns observed
- File naming conventions
- Dependencies discovered

Storage: `~claude/projects/<project>/memory/`

### 3.3 Memory Retrieval

Uses **LLM-based scan** of memory-file headers, selecting up to **5 relevant files**. No embeddings, no vector similarity search. The LLM reads headers and picks what's relevant for the current context.

### 3.4 Session Persistence

Three channels:
1. **Append-only JSONL transcripts** — full conversation history
2. **Global prompt history** — cross-session command history
3. **Subagent sidechains** — isolated sub-agent transcripts

**Chain patching**: Compact boundaries record `headUuid/anchorUuid/tailUuid`. Session loader patches message chain at read time. Nothing is destructively edited on disk.

**Permissions never restored on resume** — trust is re-established per session.

### 3.5 Context Assembly Order

9 ordered sources build the context window:
1. System prompt (base instructions)
2. CLAUDE.md hierarchy (4 levels)
3. Git status
4. Date/time
5. Previous conversation history
6. Tool results
7. Memory file contents (up to 5 selected files)
8. Skill attachments
9. Live context (cwd, env vars)

---

## 4. Tool System Design

### 4.1 Tool Interface (`Tool.ts`)

Every tool is a TypeScript structural type (protocol, not class hierarchy):

```typescript
export type Tool<Input, Output, P extends ToolProgressData> = {
  name: string                 // primary identifier
  aliases?: string[]           // legacy names for backward compat
  inputSchema: Input           // Zod schema — source of truth for validation
  maxResultSizeChars: number   // overflow → persist to disk

  call(args, context, canUseTool, parentMessage, onProgress?): Promise<ToolResult<Output>>
  checkPermissions(input, context): Promise<PermissionResult>
  
  isConcurrencySafe(input): boolean  // per-call, not per-tool-type
  isReadOnly(input): boolean
  isDestructive?(input): boolean
  
  // Optional: UI rendering, deferred loading, search indexing
  prompt(options): Promise<string>   // Dynamic tool description for model
  renderToolUseMessage?()            // React node while streaming
  renderToolResultMessage?()         // React node for transcript
  shouldDefer? / alwaysLoad?         // ToolSearch lazy-loading
  toAutoClassifierInput?()           // Security classifier input
}
```

### 4.2 `buildTool()` — Fail-Closed Defaults

```typescript
const TOOL_DEFAULTS = {
  isEnabled:         () => true,
  isConcurrencySafe: () => false,   // conservative: assume state mutation
  isReadOnly:        () => false,   // conservative: assume write
  isDestructive:     () => false,
  checkPermissions:  (input) => Promise.resolve({ behavior: 'allow', updatedInput: input }),
  toAutoClassifierInput: () => '',  // skip security classifier
}
```

An incomplete tool definition fails safe, not open.

### 4.3 4-Stage Tool Assembly Pipeline

1. **Registry** — `getAllBaseTools()` returns all ~54 tools, conditionally gated by feature flags, env vars, and user types
2. **Filtering** — `getTools()` strips denied tools; deferred tools filtered unless discovered
3. **Schema rendering** — `toolToAPISchema()` calls each tool's `prompt()` method, converts Zod to JSON Schema, cached per session
4. **Caching** — `toolSchemaCache.ts` locks rendered schema at first render, even if GrowthBook flags flip

### 4.4 Tool Pool Assembly (5-step)

```
Base enumeration (up to 54 tools)
  → Mode filtering (simple mode keeps only Bash, Read, Edit)
  → Deny pre-filtering (permission rules)
  → MCP integration (external tools appended)
  → Deduplication (sorted: built-ins alpha, then MCP alpha for cache stability)
```

### 4.5 Tool Execution Pipeline

For each `tool_use` block:
1. Find tool by name (with alias fallback)
2. Validate input (Zod `safeParse` → semantic validation)
3. Check permissions:
   - Rule match → allow/deny
   - Mode check → auto/ask
   - PreToolUse hooks
   - Auto-classifier (separate LLM call, speculative for Bash)
   - Interactive prompt to user
4. Execute `tool.call()`
5. Run PostToolUse hooks
6. Yield result

### 4.6 Concurrency Model

**Partitioning algorithm**:
- Consecutive `isConcurrencySafe === true` tools → batched, run in parallel
- Any non-safe tool → runs alone, serially
- Default max concurrency: 10 (`CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY`)

**StreamingToolExecutor**: starts executing tools as their blocks stream in from the API, before the model's full response finishes.

**Sibling abort**: Only Bash errors cascade to sibling tools (Bash commands often have implicit dependency chains). Read/WebFetch failures are treated as independent.

**In-order result emission**: Even though tools execute concurrently, results are emitted in the order the model requested them (required for `tool_result` message pairing by ID).

### 4.7 Tool Categories (54 Total)

**Always included**: Agent, TaskOutput, Bash, ExitPlanMode, FileRead, FileEdit, FileWrite, NotebookEdit, WebFetch, TodoWrite, WebSearch, TaskStop, AskUserQuestion, Skill, EnterPlanMode, SendMessage, Brief (SendUserMessage), ListMcpResources, ReadMcpResource

**Conditionally included**: Glob/Grep (when embedded search not available), Config (internal), Tungsten (internal), SuggestBackgroundPR (internal), WebBrowser, TaskCreate/Get/Update/List (TodoV2), OverflowTest, CtxInspect (context collapse), TerminalCapture, LSP, EnterWorktree/ExitWorktree, ListPeers, TeamCreate/TeamDelete (agent swarms), VerifyPlanExecution, REPL (internal), Workflow, Sleep, CronCreate/Delete/List, RemoteTrigger, Monitor, SendUserFile, PushNotification, SubscribePR, PowerShell, Snip, ToolSearch

**Simple mode** (`CLAUDE_CODE_SIMPLE=1`): Only Bash, FileRead, FileEdit — the irreducible minimum.

### 4.8 Dynamic Tool Prompts

Each tool has its own `prompt.ts` that dynamically generates instructions:
- **BashTool**: Most complex — composable sections, sandbox config serialized as JSON, different git manuals for internal vs external users
- **FileReadTool**: Template function with runtime file size limits, PDF support conditional
- **FileEditTool**: Adapts to user type (internal users get minimal-uniqueness hint)
- **GrepTool**: Static string — no conditionals
- **WebFetchTool**: Quotes capped at 125 characters on non-preapproved domains
- **WebSearchTool**: Current year/month hardcoded into every search
- **AgentTool**: Agent list moved to system-reminder attachment (saved 10.2% of fleet cache tokens)

---

## 5. Context Window Management & Compaction

### 5.1 The Problem

Claude Code has a **200K token context window** (up to 1M on Claude 4.6 series). Performance degrades around **147K-152K tokens**. Auto-compaction triggers at **64-75% capacity**. The per-turn cost compounds as context grows — not just linearly, but rapidly in agentic or file-heavy sessions.

### 5.2 5-Layer Compaction (Graduated Lazy-Degradation)

Applied **before every model call**, cheapest first:

#### Layer 1: Tool Result Budget (cost: free)
- Caps oversized tool results (file reads, search results spanning thousands of lines)
- Trims to budget before including in context

#### Layer 2: Snip Compact (cost: free, gated by `HISTORY_SNIP` flag)
- Drops old messages wholesale — no summarization, just discards
- Primarily used in headless/background sessions
- Tracks `snipTokensFreed` to inform Auto-Compact's threshold check

#### Layer 3: Microcompact (cost: free, per-turn)
- Selectively clears individual tool results with **prompt-cache awareness**
- Replaces old tool outputs with short placeholders: `"[Old tool result content cleared]"`
- Example: file contents read 10 turns ago that Claude has already digested
- Cache-aware: preserves cache boundaries when possible

#### Layer 4: Context Collapse (cost: free, gated by `CONTEXT_COLLAPSE` flag)
- Creates a collapsed **read-only view** of message blocks — non-destructive
- Originals are NOT touched; collapse is a projection
- Runs before Auto-Compact so that if collapse gets under threshold, Auto-Compact is a no-op

#### Layer 5: Auto-Compact (cost: 1 API call, threshold-based)
- **Last resort**: summarizes the entire conversation via LLM call
- Two strategies:
  - **Strategy A: Session Memory Compaction (SM-Compact)** — first tries to compact using session memory files
  - **Strategy B: Full Conversation Compaction** — fallback: summarizes entire conversation
- Strips images, summarizes, then retries
- "Strip retry": if summary itself is too large, removes media and tries once more

### 5.3 Reactive vs. Proactive Compaction

- **Proactive**: Runs the 5-layer pipeline before every API call
- **Reactive**: Triggered on 413 (Prompt Too Long) errors — commits pending collapse candidates, then does reactive compact as last resort

### 5.4 Lifecycle of a Long Conversation

```
Turn 1: Fresh context, ~5K tokens
Turn 10: ~40K tokens, microcompact starts clearing old tool results
Turn 30: ~90K tokens, Context Collapse kicks in
Turn 50: ~130K tokens, approaching 64% threshold
Turn 60: Auto-Compact fires → conversation summarized to ~20K tokens
Turn 61+: Fresh context grows again from summary base
```

### 5.5 Post-Compaction Hooks

After compaction fires, Claude can re-read CLAUDE.md and memory files to restore critical context that may have been lost in the summary.

---

## 6. Sub-Agent Architecture

### 6.1 6 Built-in Sub-Agent Types

| Type | Purpose |
|---|---|
| **Explore** | Code navigation, file search |
| **Plan** | Planning complex tasks |
| **General-purpose** | Arbitrary task execution |
| **Guide** | User guidance / instructions |
| **Verification** | Verify work / test results |
| **Statusline** | Status reporting |

### 6.2 Custom Sub-Agents

Defined as `.claude/agents/*.md` files with YAML frontmatter:

```yaml
---
name: implementer-tester
description: "Implements and tests code changes"
tools: [Bash, FileRead, FileEdit, FileWrite, Grep, Glob]
disallowedTools: [WebFetch]  # optional denylist
model: claude-sonnet-4-20250514  # optional model override
effort: high  # optional effort level
permissionMode: auto  # optional permission mode
mcpServers: [...]  # optional MCP servers
hooks: [...]  # optional hooks
maxTurns: 20  # optional turn limit
skills: [...]  # optional skill loading
memory: project  # optional memory scope
background: false  # optional background execution
isolation: worktree  # optional: worktree | remote | in-process
---
Your system prompt instructions here...
```

### 6.3 Sidechain Transcripts

**Critical design**: Sub-agents operate in **isolated context windows**. Only **summaries** return to the parent agent. The parent's context is protected from sub-agent verbosity.

Three isolation modes:
- **Worktree** — separate git worktree
- **Remote** — separate process/machine
- **In-process** — shared process, isolated context

Coordination via **POSIX `flock()`**.

### 6.4 SkillTool vs. AgentTool

| | SkillTool | AgentTool |
|---|---|---|
| **Context** | Injects into current context | Spawns isolated context |
| **Cost** | Cheap (no new window) | Expensive (new window) |
| **Use case** | Quick instructions, patterns | Complex multi-step work |
| **Risk** | Can pollute parent context | Context explosion prevented |

### 6.5 Permission Inheritance

Sub-agent `permissionMode` applies UNLESS parent is in `bypassPermissions`/`acceptEdits`/`auto` (explicit user decisions always take precedence).

### 6.6 Context Savings

The primary benefit of sub-agents is **context window savings**:
- Research/exploration can consume 50K+ tokens
- Delegated to a sub-agent, only the summary (~500-2K tokens) returns to parent
- Parent context stays clean for the main task

---

## 7. Permission & Security Model

### 7.1 7 Permission Modes (Graduated Trust Spectrum)

| Mode | Behavior |
|---|---|
| `plan` | Read-only, no execution |
| `default` | Ask permission for modifications |
| `acceptEdits` | Auto-approve file edits, ask for commands |
| `auto` | ML classifier decides (93% approval rate) |
| `dontAsk` | Skip prompts, use cached decisions |
| `bypassPermissions` | No permission checks at all |
| `bubble` (internal) | Internal testing mode |

**Deny-first rule**: A broad deny always overrides a narrow allow. Strictest rule wins.

### 7.2 7 Independent Safety Layers

1. **Tool pre-filtering** — strip denied tools before model sees them
2. **Input validation** — Zod schema validation
3. **PreToolUse hooks** — user-defined validation logic
4. **Deny-first rule evaluation** — permission rules engine
5. **Interactive permission prompts** — user approval
6. **Shell sandboxing** — filesystem + network isolation
7. **Hook interception** — post-execution validation

### 7.3 Auto-Mode Classifier (`yoloClassifier.ts`)

- Separate LLM call with internal/external permission templates
- Two-stage: fast-filter + chain-of-thought
- 93% prompt-approval rate in production
- Speculative execution for Bash commands (starts classifier before hooks run)

### 7.4 Sandboxing Architecture

Built on OS-level primitives:
- **macOS**: Seatbelt profile
- **Linux**: Bubblewrap

Two boundaries:
1. **Filesystem isolation** — read/write only to project directory, blocked from system files
2. **Network isolation** — only approved servers via unix domain socket → proxy

Both are required: without network isolation, compromised agent could exfiltrate SSH keys; without filesystem isolation, agent could escape sandbox.

Result: **84% reduction in permission prompts** in internal usage.

### 7.5 Pre-Trust Execution Window (Vulnerability)

2 patched CVEs share this root cause: hooks and MCP servers execute during initialization **before** the trust dialog appears, creating a structurally privileged attack window outside the deny-first pipeline.

### 7.6 Authorization Pipeline

```
Pre-filtering (strip denied tools)
  → PreToolUse hooks
  → Deny-first rule evaluation
  → Permission handler (4 branches: coordinator, swarm worker, speculative classifier, interactive)
```

### 7.7 Shared Failure Modes

Per-subcommand parsing causes event-loop starvation — commands exceeding **50 subcommands** bypass security analysis entirely to prevent the REPL from freezing. This means defense-in-depth degrades when layers share performance constraints.

---

## 8. Self-Improvement & Learning Mechanisms

### 8.1 Auto-Memory (Built-in)

Claude Code has a built-in **auto-memory** system that accumulates knowledge across sessions:
- Build commands discovered
- Debugging insights and resolutions
- Architecture notes
- Code patterns and conventions
- File naming patterns observed
- Dependencies discovered

Storage: `~/.claude/projects/<project>/memory/` — plain Markdown files, fully inspectable and version-controllable.

### 8.2 Memory Hierarchy for Learning

The memory hierarchy supports progressive learning:
1. **Session memory** — automatic, per-session
2. **Auto-memory files** — cross-session, Claude-authored
3. **CLAUDE.md** — human-authored, persistent instructions
4. **`.claude/rules/*.md`** — project-level rules
5. **Skills** (`.claude/skills/`) — reusable procedures with optional `learnings.md`

### 8.3 Community Self-Improvement Patterns

While Claude Code's built-in self-improvement is primarily through auto-memory, the community has developed patterns:

**Learnings.md pattern**: Add a `learnings.md` file to any skill that captures what worked, what failed, and what to do differently — improving automatically over sessions.

**Reflect systems**: Community-built systems that enable automatic skill improvement through user corrections — maintaining external memory files that provide accumulated context at runtime without touching model weights.

**Self-improvement loops**: Skills that enforce a review cycle: "did we learn anything that should persist?" after each task, promoting findings to project memory.

### 8.4 Key Limitation

Self-improvement in Claude Code is **context-based**, not weight-based. It does not fine-tune the model. Instead, it maintains external memory files that provide accumulated context at runtime. The model itself starts fresh each session; "learning" is the progressive accumulation of memory files that get loaded at session start.

---

## 9. Architecture Diagrams

### 9.1 High-Level Component Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        ENTRY POINTS                             │
│  CLI (cli.tsx) │ MCP (--mcp) │ SDK │ Server (HTTP) │ Daemon    │
└────────┬────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────┐  ┌───────────┐  ┌──────────────────┐
│   BOOTSTRAP &   │  │   SETUP   │  │  COMMAND &       │
│   CONFIGURATION │  │           │  │  ROUTING         │
│ • init()        │  │ • cwd     │  │ • getCommands    │
│ • configs       │  │ • hooks   │  │ • agents         │
│ • telemetry     │  │ • worktree│  │ • skills         │
│ • auth prefetch │  │ • plugins │  │ • plugins        │
└─────────────────┘  └───────────┘  └────────┬─────────┘
                                               │
                                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                     UI LAYER (Ink/React TUI)                    │
│  App.tsx → REPL.tsx → Messages + PromptInput + StatusLine      │
└──────────────────────────┬──────────────────────────────────────┘
                           │ user submits message
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                     QUERY ENGINE LAYER                          │
│  QueryEngine.ts → query.ts (queryLoop async generator)          │
│  → Claude API (streaming) → Tool System → Compact Service      │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                        TOOL SYSTEM                              │
│  File │ Shell │ Web │ Agent │ Plan │ Task │ MCP │ System Tools  │
│  ──────────────────── PERMISSION LAYER ──────────────────────── │
│  Rules → Tool Logic → Mode Check → Classifier → User Ask       │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                    SERVICES & STATE                             │
│  AppState │ API Client │ Analytics │ Session Memory │ MCP      │
│  Plugin Mgr │ Hooks │ History (JSONL) │ Settings │ Token Est.  │
└─────────────────────────────────────────────────────────────────┘
```

### 9.2 ReAct Loop Flow

```
User Message
    │
    ▼
┌─────────────┐
│ QueryEngine  │
│ .submit()    │
└──────┬───────┘
       │
       ▼
┌──────────────────────────────────────────┐
│ PREPARE                                   │
│ 1. Build system prompt                    │
│ 2. Gather user context (CLAUDE.md, etc.)  │
│ 3. Normalize message history              │
│ 4. Apply 5-layer compaction               │
└──────────────┬───────────────────────────┘
               │
               ▼
┌──────────────────────────────────────────┐
│ CALL CLAUDE API (streaming)              │
│ ┌──────────────────────────────────────┐ │
│ │ for await (event of stream) {        │ │
│ │   yield event → UI renders it        │ │
│ │   StreamingToolExecutor starts tools │ │
│ │ }                                    │ │
│ └──────────────────────────────────────┘ │
└──────────────┬───────────────────────────┘
               │
       ┌───────┴────────┐
       │ Tool calls?    │
       │                │
    NO │             YES│
       ▼                ▼
  ┌──────────┐  ┌───────────────────────────┐
  │  DONE    │  │ EXECUTE TOOLS             │
  │          │  │ 1. Find tool              │
  │ Terminal │  │ 2. Validate input (Zod)   │
  │ {reason} │  │ 3. Check permissions      │
  └──────────┘  │ 4. Execute tool.call()    │
                │ 5. Yield result           │
                │ Concurrency:              │
                │ • Read-only → parallel    │
                │ • Write → sequential      │
                └───────────┬───────────────┘
                            │
                            │ tool results added to history
                            ▼
                     ┌──────────────┐
                     │  LOOP BACK   │
                     │  → CALL API  │
                     └──────────────┘
```

### 9.3 Compaction Pipeline

```
Before every model call:

   ┌────────────────────────┐
   │ Tool Result Budget     │ ← Free: Cap oversized results
   └────────────┬───────────┘
                ▼
   ┌────────────────────────┐
   │ Snip Compact           │ ← Free: Drop old messages wholesale
   └────────────┬───────────┘
                ▼
   ┌────────────────────────┐
   │ Microcompact           │ ← Free: Selective tool result clearing
   └────────────┬───────────┘   (cache-aware)
                ▼
   ┌────────────────────────┐
   │ Context Collapse       │ ← Free: Non-destructive read-only projection
   └────────────┬───────────┘
                ▼
   ┌────────────────────────┐
   │ Auto-Compact           │ ← 1 API call: Full LLM summarization
   │ Strategy A: SM-Compact │   (last resort)
   │ Strategy B: Full Conv  │
   └────────────────────────┘
```

### 9.4 7-Layer Security Model

```
┌──────────────────────────────────────────────┐
│ 1. Tool Pre-filtering (strip denied tools)   │
│ 2. Input Validation (Zod schemas)            │
│ 3. PreToolUse Hooks                          │
│ 4. Deny-First Rule Engine                    │
│ 5. Auto-Classifier (separate LLM)            │
│ 6. Interactive Permission Prompts            │
│ 7. Shell Sandboxing (filesystem + network)   │
└──────────────────────────────────────────────┘
        ↓ Deny-first: broad deny > narrow allow
        ↓ Strictest rule wins
        ↓ Permissions never restored on resume
```

---

## 10. Key Constants & Metrics

| Constant | Value | Notes |
|---|---|---|
| Context window | 200K tokens (1M on Claude 4.6) | Performance degrades at 147-152K |
| Auto-compact threshold | 64-75% capacity | Triggers proactively |
| Max output tokens escalation | 8K → 64K | `ESCALATED_MAX_TOKENS` |
| Max recovery attempts | 3 | `maxOutputTokensRecoveryCount` |
| Diminishing returns threshold | 500 tokens / 3 consecutive | "Spinning wheels" detection |
| Budget consumption limit | 90% | Stop before 100% |
| Tool max concurrency | 10 | `CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY` |
| Memory file selection | Up to 5 | LLM-based header scan |
| Sub-agent isolation | 3 modes | worktree, remote, in-process |
| Permission modes | 7 | plan → default → acceptEdits → auto → dontAsk → bypassPermissions → bubble |
| Safety layers | 7 | Defense in depth |
| Compaction layers | 5 | Graduated lazy-degradation |
| Hook events | 27 | Across 5 categories |
| Extension mechanisms | 4 | Hooks → Skills → Plugins → MCP |
| Plugin component types | 10 | commands, agents, skills, hooks, etc. |
| Tool pool size | Up to 54 | Conditional on features and environment |
| Codebase size | ~1,884 files, ~512K LOC | v2.1.88 |
| AI decision logic | ~1.6% | 98.4% is deterministic infrastructure |
| Cache TTL | 5 minutes | Prompt prefix cache |
| Agent list cache savings | 10.2% | Fleet cache_creation tokens |
| WebFetch quote cap | 125 characters | Non-preapproved domains only |
| Sandbox permission reduction | 84% | Internal usage metric |
| Auto-approve rate | 93% | Permission classifier accuracy |
| File read max lines | `MAX_LINES_TO_READ` | Configurable |
| Subcommand bypass threshold | 50 | Commands exceeding this bypass security analysis |

---

## Sources

1. **VILA-Lab/Dive-into-Claude-Code** — Systematic source-level analysis (arXiv:2604.14228)
2. **Zain Hasan — Inside Claude Code: Architecture Deep Dive** — Visual and narrative guide
3. **bits-bytes-nn — Claude Code Architecture Analysis** — npm source map leak analysis
4. **Anthropic Engineering — Beyond Permission Prompts** — Official sandboxing documentation
5. **shareAI-lab/learn-claude-code** — "Bash is all you need" agent harness tutorial
6. **markdown.engineering — Claude Code Course** — Source deep dive lessons
7. **Code Pointer — Tool Design for AI Agents** — 50+ tool analysis
8. **ComeOnOliver/claude-code-analysis** — Reverse-engineering analysis
9. **Anthropic Official Documentation** — code.claude.com/docs
10. **FlorianBruniaux/claude-code-ultimate-guide** — Community comprehensive guide
