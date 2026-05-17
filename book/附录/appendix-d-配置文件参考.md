# 附录 D：配置文件完整参考

> BaoClaw 的配置分为用户级（全局）和项目级两层。用户级配置在 `~/.baoclaw/`，项目级配置在 `<项目目录>/.baoclaw/`。
>
> 完整的在线文档：[github.com/baohx/BaoClaw](https://github.com/baohx/BaoClaw)

## 目录结构总览

```
~/.baoclaw/                          # 用户级（全局，跨项目）
├── config.json                      # 主配置文件
├── memory.jsonl                     # 全局记忆（项目级不存在时的 fallback）
├── cron.json                        # 定时任务配置
├── sessions/                        # 会话记录（按项目 cwd hash 分文件）
│   └── {cwd_hash}-{uuid}.jsonl
├── skills/                          # 个人技能（跨项目生效）
│   └── my-skill.md
├── plugins/                         # 用户级插件
│   └── my-plugin/
│       ├── skills/
│       └── mcp.json
├── mcp.json                         # 用户级 MCP 服务器
├── mcp-auth/                        # MCP OAuth 令牌存储
├── models/                          # 本地模型文件（如 whisper）
├── telemetry/                       # 遥测事件（仅本地）
├── evolution/                       # 自我进化数据
│   ├── trajectories.jsonl           # 交互轨迹（用于 RLHF）
│   ├── candidates/                  # 自动提取的 skill 候选
│   └── training_export.jsonl        # 导出的训练数据
├── telegram-gateway.pid             # Telegram 网关 PID
└── telegram-gateway.log             # Telegram 网关日志

<项目>/.baoclaw/                     # 项目级
├── BAOCLAW.md                       # 项目指令 → 注入系统提示词
├── mcp.json                         # 项目 MCP 服务器
├── mcp.local.json                   # 本地 MCP 覆盖（应 gitignore）
├── memory.jsonl                     # 项目级记忆
├── skills/                          # 项目专属技能
├── plugins/                         # 项目级插件
├── backups/                         # 文件编辑前的备份
└── todo.json                        # 项目待办列表
```

## 各文件详解

### config.json — 主配置

路径：`~/.baoclaw/config.json`

```json
{
  "model": "claude-sonnet-4-20250514",
  "fallback_models": ["claude-3-5-haiku-20241022"],
  "max_retries_per_model": 2,
  "api_type": "anthropic",
  "openai_base_url": null,
  "telegram": {
    "token": "123456:ABC-DEF...",
    "allowedChatIds": [12345678]
  }
}
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `model` | string | `claude-sonnet-4-20250514` | 主模型 |
| `fallback_models` | string[] | `[]` | 限流时的降级模型链 |
| `max_retries_per_model` | number | `2` | 每个模型的重试次数 |
| `api_type` | string | `"anthropic"` | `"anthropic"` 或 `"openai"` |
| `openai_base_url` | string? | `null` | OpenAI 兼容 API 地址 |
| `telegram.token` | string | — | Telegram Bot Token |
| `telegram.allowedChatIds` | number[] | `[]` | 允许的聊天 ID（空=全部允许） |

环境变量覆盖：
- `ANTHROPIC_API_KEY` — API 密钥（必需）
- `ANTHROPIC_MODEL` — 覆盖 `model` 字段
- `ANTHROPIC_BASE_URL` — 覆盖 `openai_base_url`
- `BRAVE_SEARCH_API_KEY` — Web 搜索 API 密钥

### BAOCLAW.md — 项目指令

路径：`<项目>/.baoclaw/BAOCLAW.md` 或 `<项目>/BAOCLAW.md`（前者优先）

内容会被注入到每次对话的系统提示词中。写任何你希望 Agent 了解的项目信息。

```markdown
# 我的项目

这是一个 Python Web 应用，使用 FastAPI + SQLAlchemy。

## 规范
- 所有函数必须有类型注解
- 测试放在 tests/ 目录
- 使用 pytest
- 数据库迁移用 alembic

## 重要文件
- src/main.py — 入口
- src/models/ — 数据模型
- src/api/ — API 路由
```

### mcp.json — MCP 服务器配置

路径：`~/.baoclaw/mcp.json`（用户级）和 `<项目>/.baoclaw/mcp.json`（项目级，覆盖用户级）

```json
{
  "mcpServers": {
    "sqlite": {
      "command": "uvx",
      "args": ["mcp-server-sqlite", "--db-path", "./data.db"],
      "env": {},
      "disabled": false
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_..."
      }
    }
  }
}
```

`mcp.local.json` — 同样格式，用于本地覆盖（应加入 `.gitignore`）。

### cron.json — 定时任务

路径：`~/.baoclaw/cron.json`（通过 `/cron` 命令管理，daemon 运行时勿手动编辑）

```json
[
  {
    "id": "a1b2c3d4",
    "name": "每日 git 总结",
    "prompt": "总结昨天这个项目的 git 提交",
    "schedule": "daily 09:00",
    "cwd": "/home/user/my-project",
    "enabled": true,
    "created_at": "2026-04-14T10:00:00Z",
    "last_run": "2026-04-14T09:00:15Z",
    "last_result": "3 个提交：修复登录 bug、添加测试..."
  }
]
```

调度格式：`every 30m`、`every 2h`、`daily 09:00`、`weekly mon 09:00`

### skills/*.md — 技能文件

路径：`~/.baoclaw/skills/`（个人级）和 `<项目>/.baoclaw/skills/`（项目级）

```markdown
---
description: 代码审查清单
created_by: evolution
version: 2
---

# 代码审查

审查代码时：
1. 检查安全问题（SQL 注入、XSS 等）
2. 检查错误处理
3. 检查命名规范
4. 建议性能优化
5. 以清单形式输出 ✅/❌
```

技能可以手动创建，也可以由进化引擎自动生成。

### memory.jsonl — 记忆存储

路径：`~/.baoclaw/memory.jsonl`（全局）和 `<项目>/.baoclaw/memory.jsonl`（项目级）

每行一个 JSON 对象：

```json
{"id":"a1b2c3d4","content":"用户偏好中文输出","category":"preference","created_at":"2026-04-14T10:00:00Z","source":"user"}
{"id":"e5f6g7h8","content":"项目使用 PostgreSQL 数据库","category":"fact","created_at":"2026-04-14T11:00:00Z","source":"agent"}
```

通过 `/memory` 命令管理。

### todo.json — 项目待办

路径：`<项目>/.baoclaw/todo.json`

```json
[
  {"text":"实现用户认证","completed":false,"priority":"high","created_at":"2026-04-14T10:00:00Z"},
  {"text":"写单元测试","completed":true,"priority":"medium","created_at":"2026-04-14T11:00:00Z"}
]
```

### sessions/{hash}-{uuid}.jsonl — 会话记录

路径：`~/.baoclaw/sessions/`

文件名格式：`{cwd_hash前8位}-{uuid前8位}.jsonl`，同一个项目目录始终使用同一个文件。

每行一个 JSON 对象，记录完整的对话历史（用户消息、助手回复、工具调用和结果）。

### evolution/ — 进化数据

路径：`~/.baoclaw/evolution/`（全局，跨项目）

- `trajectories.jsonl` — 每次交互的完整轨迹
- `candidates/` — 自动提取的 skill 候选（JSON 文件）
- `training_export.jsonl` — 导出的 RLHF 训练数据

## 优先级规则

| 配置类型 | 优先级（高→低） |
|----------|-----------------|
| MCP 服务器 | `mcp.local.json` > 项目 `mcp.json` > 插件 MCP > 用户 `mcp.json` |
| 技能 | 项目 `skills/` + 用户 `skills/` + 插件 skills（全部加载） |
| 记忆 | 项目 `memory.jsonl`（有则用）> 全局 `memory.jsonl` |
| 项目指令 | `.baoclaw/BAOCLAW.md` > 根目录 `BAOCLAW.md` |
| 模型 | `ANTHROPIC_MODEL` 环境变量 > `config.json` 中的 `model` |
