# 附录 A：BaoClaw 完整架构图

## 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                      Client Layer                            │
│                                                              │
│  ┌──────────────┐  ┌────────────────┐  ┌─────────────────┐ │
│  │  Terminal CLI │  │   Telegram     │  │  Future Clients │ │
│  │  (TypeScript) │  │   Gateway      │  │  (WhatsApp,     │ │
│  │              │  │  (TypeScript)   │  │   Web UI, ...)  │ │
│  │  - TUI 渲染   │  │  - Bot API     │  │                 │ │
│  │  - 命令路由   │  │  - HTML 格式化  │  │                 │ │
│  │  - 权限交互   │  │  - 图片发送    │  │                 │ │
│  └──────┬───────┘  └───────┬────────┘  └────────┬────────┘ │
│         │                  │                     │           │
│         └──────────────────┼─────────────────────┘           │
│                            │                                 │
│              IPC: JSON-RPC 2.0 / Unix Domain Socket          │
│                            │                                 │
├────────────────────────────┼─────────────────────────────────┤
│                            ▼                                 │
│                    Daemon (Rust)                              │
│                                                              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                    SharedState                         │  │
│  │                                                        │  │
│  │  ┌──────────────┐  ┌───────────────┐  ┌────────────┐ │  │
│  │  │ engine_tools │  │ SessionRegistry│  │ MemoryStore│ │  │
│  │  │ (14 builtin  │  │ (shared mode) │  │ (JSONL)    │ │  │
│  │  │  + MCP tools)│  └───────────────┘  └────────────┘ │  │
│  │  └──────────────┘  ┌───────────────┐  ┌────────────┐ │  │
│  │  ┌──────────────┐  │PermissionGate │  │ TaskManager│ │  │
│  │  │ skill_prompt │  └───────────────┘  └────────────┘ │  │
│  │  │ (Skills +    │  ┌───────────────┐  ┌────────────┐ │  │
│  │  │  Memory)     │  │ StateManager  │  │ CostTracker│ │  │
│  │  └──────────────┘  └───────────────┘  └────────────┘ │  │
│  └───────────────────────────────────────────────────────┘  │
│                            │                                 │
│                            ▼                                 │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                   QueryEngine                          │  │
│  │                                                        │  │
│  │  messages: Vec<Message>  ←→  LLM API (streaming)      │  │
│  │                          ←→  Tool Executor             │  │
│  │                          ←→  TranscriptWriter          │  │
│  │                                                        │  │
│  │  ReAct Loop:                                           │  │
│  │  [思考 → 工具调用 → 观察结果 → 再思考]*  → 最终回复    │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                   External Services                    │  │
│  │                                                        │  │
│  │  ┌──────────┐  ┌──────────────────┐  ┌─────────────┐ │  │
│  │  │ LLM API  │  │ MCP Servers      │  │ Git         │ │  │
│  │  │ (GLM-5,  │  │ (computer-ctrl,  │  │ (diff,      │ │  │
│  │  │  Claude)  │  │  custom, ...)    │  │  commit)    │ │  │
│  │  └──────────┘  └──────────────────┘  └─────────────┘ │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## 文件系统布局

```
~/.baoclaw/
├── bin/claude-core          # Rust daemon 二进制
├── config.json              # 全局配置（model, fallback_models）
├── mcp.json                 # MCP 服务器配置
├── memory.jsonl             # 长期记忆
├── skills/                  # 全局 Skills
│   ├── desktop-launch-app/SKILL.md
│   └── debate-competition/SKILL.md
├── plugins/                 # 全局 Plugins
│   └── test-plugin/
│       ├── plugin.json
│       ├── skills/
│       └── mcp.json
├── sessions/                # 会话 Transcript
│   └── <session-id>.jsonl
├── ts-ipc/                  # CLI 客户端
│   ├── cli.ts
│   └── markdownRenderer.ts
└── baoclaw-telegram/        # Telegram 网关
    └── src/
        ├── gateway.ts
        └── commands.ts

<project>/
├── BAOCLAW.md               # 项目级指令
└── .baoclaw/
    ├── mcp.json             # 项目级 MCP
    ├── skills/              # 项目级 Skills
    └── todo.json            # TODO 列表
```
