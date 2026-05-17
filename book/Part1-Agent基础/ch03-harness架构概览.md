# 第 3 章：Agent Harness 架构概览

## 3.1 为什么需要 Harness

直接调用 LLM API 写一个 chatbot 很简单。但要构建一个生产级的 Agent 系统，你需要解决大量"API 之外"的问题：

- 工具怎么注册和发现？
- 多个客户端怎么连接同一个 Agent？
- 对话历史太长怎么办？
- 工具执行需要用户授权怎么处理？
- Agent 进程崩溃了怎么恢复？

Agent Harness 就是解决这些问题的框架。它是 Agent 的"运行时环境"。

## 3.2 BaoClaw 的三层架构

BaoClaw 采用经典的三层架构：

```
┌─────────────────────────────────┐
│        Client Layer             │  TypeScript
│  (CLI, Telegram, ...)           │
├─────────────────────────────────┤
│        IPC Layer                │  JSON-RPC 2.0 / UDS
│  (Protocol, Routing)            │
├─────────────────────────────────┤
│        Core Layer               │  Rust
│  (QueryEngine, Tools, Memory)   │
└─────────────────────────────────┘
```

### Client Layer（客户端层）

负责用户交互。每个客户端是一个独立进程，通过 IPC 连接到 Core。

- `ts-ipc/cli.ts` —— 终端 CLI，富文本 TUI
- `baoclaw-telegram/src/gateway.ts` —— Telegram Bot 网关

客户端是"薄"的 —— 只负责 UI 渲染和用户输入，不包含业务逻辑。

### IPC Layer（通信层）

客户端和 Core 之间的通信协议。

- 传输：Unix Domain Socket（UDS）
- 协议：JSON-RPC 2.0 + NDJSON 分帧
- 发现：`/tmp/baoclaw-sockets/` 目录下的 metadata JSON 文件

### Core Layer（核心层）

Agent 的大脑。一个 Rust 守护进程，包含：

| 组件 | 职责 |
|------|------|
| QueryEngine | ReAct 循环、LLM API 调用 |
| Tool System | 工具注册、执行、权限控制 |
| MemoryStore | 长期记忆持久化 |
| SessionRegistry | 共享会话管理 |
| TranscriptWriter | 对话记录持久化 |
| PermissionGate | 工具执行授权 |
| CostTracker | Token 计量和成本追踪 |
| TaskManager | 后台任务管理 |

## 3.3 Daemon 模式

BaoClaw 的 Core 以 Daemon（守护进程）模式运行：

```bash
# CLI 启动时自动 spawn daemon
baoclaw
  └→ spawn claude-core --daemon --cwd /path/to/project
       └→ 监听 UDS socket
       └→ 输出 SOCKET:/tmp/baoclaw-sockets/baoclaw-xxx.sock
       └→ 写入 metadata JSON
       └→ 等待客户端连接
```

Daemon 模式的优势：

1. **进程独立** —— CLI 退出不影响 daemon，Telegram 可以随时连接
2. **多客户端** —— 多个客户端可以同时连接同一个 daemon
3. **会话持久** —— 对话历史在 daemon 进程内存中，不随客户端断开而丢失
4. **资源共享** —— MCP 服务器连接、工具实例在所有客户端间共享

## 3.4 数据流

一次完整的用户交互的数据流：

```
1. 用户在终端输入 "帮我创建一个 hello.py"
2. CLI 发送 JSON-RPC: submitMessage { prompt: "帮我创建一个 hello.py" }
3. Daemon 收到请求，QueryEngine 开始 ReAct 循环
4. QueryEngine 调用 LLM API（带系统提示词 + 对话历史 + 工具定义）
5. LLM 返回 tool_use: FileWrite { path: "hello.py", content: "..." }
6. Daemon 发送 stream/event: tool_use 通知给客户端
7. QueryEngine 执行 FileWriteTool
8. Daemon 发送 stream/event: tool_result 通知给客户端
9. QueryEngine 再次调用 LLM（带工具结果）
10. LLM 返回文字回复 "已创建 hello.py"
11. Daemon 发送 stream/event: assistant_chunk 通知给客户端
12. Daemon 发送 stream/event: result 通知给客户端
13. CLI 渲染最终结果
```

如果 Telegram 也连接了同一个 daemon（共享模式），步骤 6-12 的事件会同时广播给 Telegram 客户端。

## 3.5 配置体系

BaoClaw 的配置分为多个层级：

| 层级 | 文件 | 作用域 |
|------|------|--------|
| 全局配置 | `~/.baoclaw/config.json` | 所有项目 |
| 全局 MCP | `~/.baoclaw/mcp.json` | 所有项目 |
| 全局 Skills | `~/.baoclaw/skills/` | 所有项目 |
| 全局 Memory | `~/.baoclaw/memory.jsonl` | 所有项目 |
| 项目配置 | `<cwd>/.baoclaw/` | 当前项目 |
| 项目指令 | `<cwd>/BAOCLAW.md` | 当前项目 |
| 项目 MCP | `<cwd>/.baoclaw/mcp.json` | 当前项目 |
| 项目 Skills | `<cwd>/.baoclaw/skills/` | 当前项目 |

这种分层设计让用户可以在全局设置通用偏好，在项目级设置特定规则。

## 3.6 小结

Agent Harness 的核心职责是：让 Agent 能够稳定、高效、安全地运行。BaoClaw 通过 Daemon 模式 + IPC 通信 + 分层配置实现了这一目标。接下来我们将深入每个子系统的设计细节。
