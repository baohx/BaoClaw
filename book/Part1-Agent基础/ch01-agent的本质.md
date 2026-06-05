# 第 1 章：Agent 的本质 —— 从 LLM 到 Agent

## 1.1 LLM 不是 Agent

大语言模型（LLM）本质上是一个函数：输入一段文本，输出一段文本。它没有记忆、没有工具、没有自主行动的能力。你问它"帮我创建一个文件"，它只能告诉你怎么做，但不能真的去做。

```
LLM: text → text
```

Agent 不同。Agent 是一个**循环系统**：它接收用户指令，思考需要做什么，调用工具执行动作，观察结果，然后决定下一步。这个循环一直持续，直到任务完成。

```
Agent: 指令 → [思考 → 行动 → 观察]* → 结果
```

关键区别在于：LLM 是一次性的输入输出，Agent 是持续的循环决策。

## 1.2 Agent 的三个核心能力

一个 Agent 系统需要三个核心能力：

**1. 推理（Reasoning）**

Agent 需要理解用户意图，分解任务，制定计划。这部分由 LLM 提供。

**2. 行动（Action）**

Agent 需要能够执行动作 —— 读写文件、运行命令、调用 API。这部分由工具系统（Tool System）提供。

**3. 记忆（Memory）**

Agent 需要记住之前发生了什么 —— 对话历史、用户偏好、项目上下文。这部分由记忆系统（Memory System）提供。

在 BaoClaw 中，这三个能力分别对应：

| 能力 | BaoClaw 实现 | 代码位置 |
|------|-------------|----------|
| 推理 | QueryEngine 调用 LLM API | `baoclaw-core/src/engine/query_engine.rs` |
| 行动 | Tool trait + 内置工具 + MCP | `baoclaw-core/src/tools/` |
| 记忆 | Messages + Transcript + MemoryStore | `baoclaw-core/src/engine/` |
| 上下文 | TokenCounter + ContextAllocator + Compact | `baoclaw-core/src/engine/token_counter.rs` |
| 安全 | Sandbox + PromptInjection + SubagentPolicy | `baoclaw-core/src/engine/sandbox.rs` |

## 1.3 Agent Harness 是什么

Agent Harness（Agent 运行时框架）是承载 Agent 运行的基础设施。它不是 Agent 本身，而是让 Agent 能够运行的"容器"。

类比：
- LLM 是引擎
- Agent 是驾驶员
- Agent Harness 是整辆车 —— 包括方向盘、油门、刹车、仪表盘

BaoClaw 就是一个 Agent Harness。它提供：

- **进程管理**：Daemon 模式，后台运行
- **IPC 通信**：JSON-RPC over Unix Domain Socket
- **工具注册**：Tool trait，统一的工具接口
- **上下文构建**：系统提示词、Skills、Memory 的组装
- **流式输出**：SSE 事件流，实时反馈
- **多客户端**：终端、Telegram、WhatsApp、飞书
- **会话管理**：共享会话、会话恢复、Compact
- **安全沙箱**：Bubblewrap/Docker 隔离执行
- **跨会话检索**：SQLite FTS5 全文搜索
- **自我进化**：Skill 自动提取与改进
- **Cron 调度**：定时任务执行
- **成本追踪**：Token 计量与预算控制
- **工具健康**：自动监控与降级
- **意图预测**：预加载工具提示

## 1.4 BaoClaw 的整体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                          Clients                                 │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │ Terminal │  │ Telegram │  │ WhatsApp │  │  Feishu  │        │
│  │   CLI    │  │  Gateway │  │  Gateway │  │  Gateway │        │
│  └─────┬────┘  └─────┬────┘  └─────┬────┘  └─────┬────┘        │
│        └─────────────┴─────────────┴─────────────┘              │
│                              │                                   │
│                    IPC (JSON-RPC / UDS)                          │
│                              ▼                                   │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                  Global Daemon (Rust)                       │ │
│  │                                                             │ │
│  │  ┌─────────────────────────────────────────────────────┐   │ │
│  │  │              SharedState                              │   │ │
│  │  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌─────────┐ │   │ │
│  │  │  │ Tools    │ │ Session  │ │ Memory   │ │ Cron    │ │   │ │
│  │  │  │(15+内置) │ │ Registry │ │ Store    │ │Scheduler│ │   │ │
│  │  │  │ + MCP    │ │(共享模式)│ │(长期记忆)│ │         │ │   │ │
│  │  │  └──────────┘ └──────────┘ └──────────┘ └─────────┘ │   │ │
│  │  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌─────────┐ │   │ │
│  │  │  │ Skills   │ │ Evolution│ │ Cross-   │ │ Sandbox │ │   │ │
│  │  │  │          │ │ Engine   │ │ SessionDB│ │Manager  │ │   │ │
│  │  │  └──────────┘ └──────────┘ └──────────┘ └─────────┘ │   │ │
│  │  └─────────────────────────────────────────────────────┘   │ │
│  │                              │                              │ │
│  │                              ▼                              │ │
│  │  ┌─────────────────────────────────────────────────────┐   │ │
│  │  │              QueryEngine (ReAct Loop)                │   │ │
│  │  │                                                      │   │ │
│  │  │  Messages ←→ LLM API ←→ Tools                        │   │ │
│  │  │     ↓           ↓           ↓                        │   │ │
│  │  │  Compact   Fallback   Execution                      │   │ │
│  │  │     ↓        Chain       (Sandbox)                   │   │ │
│  │  │  Context                                              │   │ │
│  │  │  Allocator                                            │   │ │
│  │  └─────────────────────────────────────────────────────┘   │ │
│  └────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              │                               │
              ▼                               ▼
     ┌────────────────┐             ┌────────────────┐
     │  Anthropic API │             │ OpenAI-compat  │
     │  (Claude)      │             │ (200+ models)  │
     └────────────────┘             └────────────────┘
```

**架构特点：**

1. **全局 Daemon**：一个守护进程管理所有项目会话
2. **项目隔离**：每个 cwd 对应独立的 Session（历史、记忆、配置）
3. **多客户端**：CLI/Telegram/WhatsApp/飞书同时连接，按 cwd 自动路由
4. **安全执行**：工具调用可选择在沙箱（Bubblewrap/Docker）中执行
5. **智能上下文**：Token 计数、自动 Compact、上下文窗口分配器

## 1.5 本书的路线图

接下来的章节将逐层深入这个架构：

- **Part 1**（本部分）：理解 Agent 的核心概念和 ReAct 循环
- **Part 2**：深入工具系统 —— 从内置工具到 MCP 协议
- **Part 3**：上下文与记忆 —— 短期、长期、项目级
- **Part 4**：IPC 与多客户端 —— Daemon 架构和共享会话
- **Part 5**：生产实践 —— 错误处理、流式输出、权限控制
- **Part 6**：高级模式 —— Computer Use、Agentic Coding

每一章都会指向 BaoClaw 的真实代码，让你不仅理解"为什么"，还能看到"怎么做"。
