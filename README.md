<div align="center">

# 🐾 BaoClaw

**The AI coding agent that remembers, evolves, and follows you everywhere.**

[English](#english) · [中文](#中文)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.5.0-blue.svg)]()

</div>

---

<a name="english"></a>

## What is BaoClaw?

BaoClaw is an open-source AI coding agent with a Rust core engine, persistent memory, cross-device session sharing, and a self-evolution loop. It runs as a daemon on your machine and connects to your terminal, Telegram, and WhatsApp — all sharing the same conversation context.

Unlike agents that forget everything when you close the window, BaoClaw builds up knowledge about you and your projects over time. The more you use it, the better it gets.

## Key Features

### 🧠 Persistent Memory
- **Project-level memory** — each project directory gets its own `memory.jsonl`
- **Global memory** — cross-project facts, preferences, and decisions in `~/.baoclaw/`
- **Long-term recall** — memories are injected into the system prompt automatically
- **Manual control** — `/memory add`, `/memory list`, `/memory delete`

### 📱 Multi-Client, Shared Sessions
- **One daemon, many clients** — CLI, Telegram bot, and WhatsApp gateway connect to the same engine
- **Shared conversation** — start a task on your laptop terminal, continue on Telegram from your phone
- **Real-time streaming** — all clients see tool calls and responses as they happen
- **Session persistence** — conversations survive daemon restarts, tied to project directories

### 🔄 Self-Evolution Engine
Inspired by [Hermes Agent](https://github.com/NousResearch/hermes-agent)'s learning loop:
- **Trajectory recording** — every interaction is logged with tools used, outcomes, and timing
- **Skill auto-generation** — complex successful tasks are extracted as reusable skill candidates
- **Self-evaluation nudge** — every 15 tasks, the agent reflects on patterns and creates/improves skills
- **User ratings** — rate interactions as good/bad to build preference data
- **RLHF data export** — export trajectories as JSONL for DPO/RLHF fine-tuning of smaller models
- **Personal evolution** — skills and trajectories are cross-project (`~/.baoclaw/evolution/`)

### 📄 Document Q&A
- **Upload files** — PDF, DOCX, and images via Telegram or CLI (`@file.pdf`)
- **Route A** — client-side text extraction (mammoth for DOCX, pdf-parse for PDF)
- **Route B** — native API document blocks (PDF sent directly to Claude/OpenAI)
- **Image understanding** — photos analyzed via multimodal API (both Anthropic and OpenAI compatible)

### 🗂️ Project-Scoped Everything
- **`/cd` command** — switch working directory at runtime, like changing projects
- **Auto-scaffold** — `.baoclaw/` directory with config files created automatically
- **Session per project** — each directory maps to its own session file
- **Auto-resume** — reconnecting to a project automatically restores conversation history
- **Project instructions** — `BAOCLAW.md` loaded into system prompt per project

### 🛠️ 15+ Built-in Tools
| Tool | Description |
|------|-------------|
| Bash | Shell commands (respects project cwd) |
| FileRead / FileWrite / FileEdit | File operations with path validation |
| Grep / Glob | Code search and file discovery |
| WebSearch | Brave Search API with retry on rate limits |
| WebFetch | Fetch and parse web pages |
| Memory | Long-term memory management |
| Agent | Sub-agent for parallel tasks |
| Evolve | Self-improvement: create/improve skills, export training data |
| Todo | Task list management |
| Notebook | Jupyter notebook editing |
| ProjectNote | Project-level notes |

### 🔌 Extensible
- **MCP support** — connect external MCP servers for additional tools
- **Skills** — markdown-based skill files loaded into system prompt
- **Plugins** — directory-based plugin system with tools, skills, and MCP configs
- **200+ LLM models** — Anthropic native + any OpenAI-compatible API (OpenRouter, Ollama, vLLM, etc.)

### 🔁 Model Fallback
- **Automatic retry** — rate-limited requests retry with exponential backoff
- **Fallback chain** — configure multiple models; if one is rate-limited, fall back to the next
- **Transparent** — CLI shows model switches in real-time

## Architecture

```
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│  Terminal    │  │  Telegram   │  │  WhatsApp   │
│  CLI (TUI)  │  │  Bot        │  │  Gateway    │
└──────┬──────┘  └──────┬──────┘  └──────┬──────┘
       │                │                │
       └────────┬───────┴────────┬───────┘
                │   Unix Socket (IPC)    │
                │   JSON-RPC 2.0 / NDJSON│
         ┌──────┴──────────────────┐
         │   BaoClaw Daemon (Rust) │
         │                         │
         │  ┌─────────────────┐    │
         │  │  Query Engine   │    │
         │  │  (Streaming)    │    │
         │  └────────┬────────┘    │
         │           │             │
         │  ┌────────┴────────┐   │
         │  │  Tool Executor  │   │
         │  │  15+ built-in   │   │
         │  │  + MCP servers  │   │
         │  └─────────────────┘   │
         │                         │
         │  ┌─────────────────┐   │
         │  │  Evolution      │   │
         │  │  Engine         │   │
         │  └─────────────────┘   │
         └─────────────────────────┘
                    │
         ┌──────────┴──────────┐
         │  Anthropic / OpenAI │
         │  Compatible API     │
         └─────────────────────┘
```

## Installation

### Prerequisites
- **Rust** (1.75+) — [rustup.rs](https://rustup.rs)
- **Node.js** (18+) — [nodejs.org](https://nodejs.org)
- An LLM API key (Anthropic, OpenRouter, or any OpenAI-compatible provider)

### Linux / macOS

```bash
git clone https://github.com/user/BaoClaw.git
cd BaoClaw
./install.sh
```

The installer builds the Rust core, installs Node.js dependencies, and creates the `baoclaw` launcher in `~/.local/bin/`.

### Windows (WSL2)

BaoClaw requires a Unix environment. On Windows, use WSL2:

```powershell
# Install WSL2 if not already installed
wsl --install

# Inside WSL2
git clone https://github.com/user/BaoClaw.git
cd BaoClaw
./install.sh
```

### Manual Setup

```bash
# 1. Build Rust core
cd baoclaw-core
cargo build --release
cd ..

# 2. Install CLI dependencies
cd ts-ipc
npm install
cd ..

# 3. Set your API key
export ANTHROPIC_API_KEY=sk-ant-...
# Or for OpenAI-compatible:
export ANTHROPIC_API_KEY=your-key
export ANTHROPIC_BASE_URL=https://your-provider.com/v1

# 4. Run
npx --prefix ts-ipc tsx ts-ipc/cli.ts
```

## Configuration

Global config: `~/.baoclaw/config.json`

```json
{
  "model": "claude-sonnet-4-20250514",
  "fallback_models": ["claude-3-5-haiku-20241022"],
  "max_retries_per_model": 2,
  "api_type": "anthropic",
  "openai_base_url": "https://your-proxy.com/v1",
  "telegram": {
    "token": "123456:ABC-DEF...",
    "allowedChatIds": [12345678]
  }
}
```

Project config: `<project>/.baoclaw/`
```
.baoclaw/
├── BAOCLAW.md          # Project instructions (injected into system prompt)
├── mcp.json            # MCP server configurations
├── memory.jsonl        # Project-level memories
└── skills/             # Project-specific skills
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `/cd <path>` | Switch project directory |
| `/tools` | List registered tools |
| `/mcp` | List MCP servers |
| `/skills` | List loaded skills |
| `/model [name]` | Show or switch model |
| `/think` | Toggle extended thinking |
| `/compact` | Compress conversation context |
| `/memory` | Manage long-term memory |
| `/diff` | Git diff summary |
| `/commit <msg>` | Stage all and commit |
| `/git` | Git status |
| `/task` | Background tasks |
| `/voice` | Voice input (whisper.cpp) |
| `/telegram` | Manage Telegram gateway |
| `@file.pdf` | Attach file for Q&A |
| `/help` | All commands |

## Telegram Setup

1. Create a bot via [@BotFather](https://t.me/BotFather)
2. Add token to `~/.baoclaw/config.json`
3. Start from CLI: `/telegram start`

Upload documents and images directly in Telegram chat — the bot extracts text and sends it to the AI.

## Self-Evolution: How It Works

```
 Use BaoClaw ──→ Trajectories recorded
                        │
                        ▼
              Complex task succeeds?
                   │          │
                  Yes         No
                   │          │
                   ▼          ▼
           Extract skill    (skip)
           candidate
                   │
                   ▼
          Every 15 tasks ──→ Self-evaluation nudge
                   │
                   ▼
          Agent creates/improves skills
                   │
                   ▼
          Skills loaded in next session
                   │
                   ▼
          Better performance ──→ Loop continues
                   │
                   ▼
          Export trajectories ──→ RLHF/DPO fine-tuning
                                  for smaller models
```

## License

MIT

---

<a name="中文"></a>

## 🐾 BaoClaw — 会记忆、会进化、跨设备的 AI 编程助手

BaoClaw 是一个开源 AI 编程 Agent，基于 Rust 核心引擎，具备持久记忆、跨设备会话共享和自我进化能力。它以守护进程方式运行，同时连接终端、Telegram 和 WhatsApp，所有客户端共享同一个对话上下文。

和那些关掉窗口就失忆的 Agent 不同，BaoClaw 会随着使用不断积累对你和你项目的了解。用得越多，越好用。

## 核心特性

### 🧠 持久记忆
- 项目级记忆 — 每个项目目录独立的 `memory.jsonl`
- 全局记忆 — 跨项目的个人偏好和决策
- 自动注入 — 记忆自动加载到系统提示词中
- 手动管理 — `/memory add`、`/memory list`、`/memory delete`

### 📱 多客户端共享会话
- 一个守护进程，多个客户端 — 终端、Telegram、WhatsApp 连接同一个引擎
- 共享对话 — 在电脑终端开始任务，用手机 Telegram 继续
- 实时流式输出 — 所有客户端同步看到工具调用和响应
- 会话持久化 — 对话在守护进程重启后自动恢复，按项目目录绑定

### 🔄 自我进化引擎
参考 [Hermes Agent](https://github.com/NousResearch/hermes-agent) 的学习循环：
- 轨迹记录 — 每次交互自动记录工具调用、结果和耗时
- Skill 自动生成 — 复杂的成功任务自动提取为可复用的 skill 候选
- 自我评估 — 每 15 个任务触发反思，创建或改进 skill
- 用户评价 — 对交互评分（good/bad），构建偏好数据
- RLHF 数据导出 — 导出轨迹数据用于小模型的 DPO/RLHF 微调
- 个人级进化 — skill 和轨迹跨项目积累（`~/.baoclaw/evolution/`）

### 📄 文档问答
- 上传文件 — 通过 Telegram 或终端（`@file.pdf`）上传 PDF、DOCX、图片
- 文本提取 — DOCX 用 mammoth，PDF 用 pdf-parse
- 原生文档 — PDF 可直接发送给 Claude API
- 图片理解 — 支持 Anthropic 和 OpenAI 兼容 API 的多模态

### 🗂️ 项目级隔离
- `/cd` 命令 — 运行时切换工作目录，相当于切换项目
- 自动初始化 — 新目录自动创建 `.baoclaw/` 配置骨架
- 项目绑定会话 — 每个目录对应独立的会话文件
- 自动恢复 — 重连时自动恢复项目的对话历史
- 项目指令 — `BAOCLAW.md` 按项目加载到系统提示词

### 🛠️ 15+ 内置工具
Bash、文件读写编辑、Grep、Glob、Web 搜索、Web 抓取、记忆管理、子 Agent、自我进化、Todo、Notebook 编辑、项目笔记等。

### 🔌 可扩展
- MCP 协议 — 连接外部 MCP 服务器获取更多工具
- Skills — Markdown 格式的技能文件，自动加载到系统提示词
- 插件系统 — 目录式插件，包含工具、技能和 MCP 配置
- 200+ 模型 — Anthropic 原生 + 任意 OpenAI 兼容 API

### 🔁 模型降级
- 自动重试 — 限流时指数退避重试
- 降级链 — 配置多个模型，限流时自动切换
- 透明提示 — 终端实时显示模型切换

## 安装

### 前置条件
- Rust (1.75+) — [rustup.rs](https://rustup.rs)
- Node.js (18+) — [nodejs.org](https://nodejs.org)
- LLM API Key（Anthropic、OpenRouter 或任意 OpenAI 兼容服务）

### Linux / macOS

```bash
git clone https://github.com/user/BaoClaw.git
cd BaoClaw
./install.sh
```

### Windows (WSL2)

```powershell
wsl --install
# 在 WSL2 中
git clone https://github.com/user/BaoClaw.git
cd BaoClaw
./install.sh
```

### 使用

```bash
export ANTHROPIC_API_KEY=sk-ant-...
baoclaw
```

OpenAI 兼容模式：
```bash
export ANTHROPIC_API_KEY=your-key
export ANTHROPIC_BASE_URL=https://your-provider.com/v1
baoclaw
```

## 自我进化：工作原理

```
使用 BaoClaw ──→ 记录交互轨迹
                      │
                      ▼
              复杂任务成功完成？
                 │          │
                是           否
                 │          │
                 ▼          ▼
          提取 skill      (跳过)
          候选
                 │
                 ▼
        每 15 个任务 ──→ 触发自我评估
                 │
                 ▼
        Agent 创建/改进 skill
                 │
                 ▼
        下次会话加载新 skill
                 │
                 ▼
        表现更好 ──→ 循环继续
                 │
                 ▼
        导出轨迹数据 ──→ RLHF/DPO 微调小模型
```

## 许可证

MIT
