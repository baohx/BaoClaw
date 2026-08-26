# WhatsApp Gateway — 需求规格

## 概述

将 BaoClaw 的 WhatsApp 网关从最小可行产品（MVP）升级为与 Telegram 网关功能对等的完整客户端。

### 背景

当前 WhatsApp 网关（`baoclaw-whatsapp/`）已完成 MVP 阶段，具备：

- Baileys 库连接 WhatsApp Web（pairing code 登录）
- 基础收发消息（白名单 + 速率限制 + 消息队列）
- Daemon IPC 连接（`submitMessage` + `stream/event` 流处理）
- 配置热加载、PID 文件、优雅关闭

但相比 Telegram 网关，缺少**命令系统、权限交互、文档处理、会话管理**等核心功能。

---

## 功能需求

### FR-1: 命令系统

WhatsApp 网关必须支持斜杠命令，与 Telegram 网关保持一致。

#### FR-1.1: 命令注册与解析

- 所有以 `/` 开头的消息视为命令
- 命令格式：`/command [args]`，如 `/tools`、`/model gpt-4o`
- 未注册的命令回复帮助信息
- 命令在消息队列之前处理（不走 `submitMessage`，直接 RPC 调用并返回结果）

#### FR-1.2: 对话类命令

| 命令              | RPC 方法        | 说明                |
| ----------------- | --------------- | ------------------- |
| `/compact`        | `compact`       | 压缩当前对话上下文  |
| `/think`          | 通过消息发送    | 触发深度思考模式    |
| `/model [name]`   | `switchModel`   | 查看/切换当前模型   |
| `/history [n]`    | `talkTail`      | 查看最近 n 条对话   |
| `/search <query>` | `searchHistory` | 搜索对话历史        |
| `/export`         | `export`        | 导出对话为 Markdown |
| `/abort`          | `abort`         | 中止当前任务        |

#### FR-1.3: 项目 & Git 命令

| 命令            | RPC 方法       | 说明          |
| --------------- | -------------- | ------------- |
| `/projects`     | `projectsList` | 列出项目      |
| `/git`          | `gitStatus`    | 查看 Git 状态 |
| `/diff`         | `gitDiff`      | 查看 Git diff |
| `/commit [msg]` | `gitCommit`    | Git 提交      |

#### FR-1.4: 工具 & 扩展命令

| 命令       | RPC 方法         | 说明            |
| ---------- | ---------------- | --------------- |
| `/tools`   | `listTools`      | 列出可用工具    |
| `/mcp`     | `listMcpServers` | 列出 MCP 服务器 |
| `/skills`  | `listSkills`     | 列出技能        |
| `/plugins` | `listPlugins`    | 列出插件        |

#### FR-1.5: 自动化命令

| 命令           | RPC 方法     | 说明         |
| -------------- | ------------ | ------------ |
| `/task <desc>` | `taskCreate` | 创建后台任务 |
| `/tasks`       | `taskList`   | 列出任务     |
| `/cron`        | `cronList`   | 列出定时任务 |

#### FR-1.6: 会话命令

| 命令      | RPC 方法                     | 说明                     |
| --------- | ---------------------------- | ------------------------ |
| `/help`   | —                            | 显示帮助信息             |
| `/status` | `projectsList` + daemon info | 显示网关和 daemon 状态   |
| `/start`  | —                            | 欢迎消息                 |
| `/clear`  | —                            | 清除本地缓存（不调 RPC） |

#### FR-1.7: Spec 命令

| 命令                  | RPC 方法     | 说明           |
| --------------------- | ------------ | -------------- |
| `/spec list`          | `specList`   | 列出 spec      |
| `/spec new <name>`    | `specNew`    | 新建 spec      |
| `/spec show <name>`   | `specShow`   | 显示 spec 内容 |
| `/spec status <name>` | `specStatus` | 显示 spec 进度 |
| `/spec run <name>`    | `specRun`    | 执行 spec 任务 |

---

### FR-2: 共享会话

- WhatsApp 网关通过 `shared_session_id: 'whatsapp'` 参数初始化 IPC 连接
- 允许 WhatsApp 与 CLI / Web / Telegram 共享同一个 daemon session
- 不同网关发来的消息按顺序处理，不会互相冲突

---

### FR-3: JID 映射修复

当前代码中 JID 映射是写死的：

```typescript
const jid = sender.replace("+", "") + "@s.whatsapp.net";
```

#### FR-3.1: 正确的 JID 映射

- 私聊：`{phone}@s.whatsapp.net`
- 群聊：`{group_id}@g.us`，群内成员为 `{phone}@s.whatsapp.net`
- 需要在 inbound 处理时保存 `sender → jid` 的映射，outbound 时使用保存的 jid 回复
- 不再从 sender phone 反推 jid

#### FR-3.2: Per-sender 状态管理

- 每个 sender 维护独立的：jid 映射、消息队列、响应累加器
- 支持同时与多个用户交互

---

### FR-4: 权限交互

#### FR-4.1: 权限请求通知

- 当 daemon 发送 `permission_request` 事件时，向用户发送权限请求消息
- 消息内容包含：工具名称、请求原因、操作描述

#### FR-4.2: 权限响应机制

- WhatsApp 不支持 inline keyboard，采用文本回复方案：
  - 发送权限请求后，设置 `pendingPermission` 状态
  - 用户回复 `yes`/`allow` → 调用 `permissionResponse` RPC（decision: "allow"）
  - 用户回复 `no`/`deny` → 调用 `permissionResponse` RPC（decision: "deny"）
  - 超时 60 秒自动 deny

---

### FR-5: 文档附件处理

#### FR-5.1: 接收文档

- 支持 WhatsApp 文档消息（PDF、DOCX）
- 收到文档后：下载到临时目录 → 通过 `docUpload` RPC 上传到 daemon
- 上传成功后将文档 ID 作为 `attachments` 参数传给 `submitMessage`

#### FR-5.2: 图片消息

- 支持接收图片消息（JPEG、PNG）
- 收到图片后：下载到临时目录 → 保存为文件
- 将文件路径传给 ImageReadTool 或直接作为上下文

---

### FR-6: 媒体消息发送

#### FR-6.1: 图片发送

- 当 daemon 返回的响应中包含图片（通过 ImageGenerator/ImageEditor 工具生成）时
- 自动将图片文件通过 WhatsApp 图片消息发送
- 在图片后附加文字说明

#### FR-6.2: 文件发送

- 当响应中包含文件路径（如 `/export` 导出的文件）时
- 通过 WhatsApp 文档消息发送文件

---

### FR-7: 健壮性增强

#### FR-7.1: Daemon 断线重连

- 当前已有基础重连逻辑，需要增强：
  - 指数退避重连（5s → 10s → 30s → 60s，上限 5 分钟）
  - 重连成功后自动恢复 stream/event 订阅
  - 重连期间的消息排队等待，不丢弃

#### FR-7.2: WhatsApp 连接断线重连

- Baileys 连接断开时自动重连
- 保持 auth state 持久化
- 最多重试 5 次，超过后退出（由 systemd/system supervisor 重启）

#### FR-7.3: 消息去重

- WhatsApp 可能重复投递消息，需要按 message ID 去重
- 维护最近 1000 条消息 ID 的 LRU 缓存

---

### FR-8: 日志与监控

#### FR-8.1: 结构化日志

- 所有日志包含：timestamp、level、component、sender（如有）、消息摘要
- 使用 JSON 格式日志（可被日志收集器解析）

#### FR-8.2: 运行状态

- `/status` 命令显示：
  - 网关运行时间、连接状态
  - Daemon 连接状态、session ID
  - WhatsApp 手机号、连接质量
  - 已处理消息数、队列深度

---

## 非功能需求

### NFR-1: 性能

- 单用户消息响应延迟 < 2 秒（不含 LLM 推理时间）
- 支持同时 5 个白名单用户并发消息

### NFR-2: 安全

- 白名单为唯一的访问控制（E.164 电话号码）
- 认证凭据（Baileys auth state）权限 0700
- IPC UDS 连接遵循 daemon 的权限模型
- 不在日志中记录完整消息内容

### NFR-3: 可靠性

- 进程崩溃后 auth state 不丢失
- Daemon 断线期间消息排队不丢弃（内存队列，上限 100 条/人）
- 优雅关闭超时 10 秒后强制退出

### NFR-4: 可维护性

- 代码结构与 Telegram 网关保持一致（gateway.ts + commands.ts）
- 命令格式化函数复用 Telegram 的逻辑，按 WhatsApp 格式调整
- 共享代码（IPC 客户端、daemon 连接器）提取为独立模块

---

## 约束

1. **WhatsApp 不支持 inline keyboard** — 权限交互必须使用文本回复方案
2. **WhatsApp 消息长度限制 65536 字符** — 超长响应需要分片
3. **Baileys 是逆向工程库** — WhatsApp 协议可能随时变化，需要跟进上游更新
4. **Pairing code 有效期短** — 首次连接需要用户在手机端操作
5. **群聊限制** — 群消息中需要 @mention 机器人才能触发（如果群策略为 allow）
