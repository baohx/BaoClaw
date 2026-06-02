# WhatsApp Gateway — 任务清单

## 前置条件

- [x] MVP 代码已存在（baoclaw-whatsapp/src/ 下 15 个文件）
- [x] Telegram 网关已实现完整功能（26 个命令、15+ RPC 方法）
- [x] IPC 协议 45 个 RPC 方法已定义

---

## Phase 1: 基础架构增强

### 1.1 创建 SenderTracker 模块 _需求: FR-3_ ✅
- [x] 新建 `senderTracker.ts`（7.6KB），实现 `SenderState` 接口和 `SenderTracker` 类
- [x] `registerSender(phone, jid, isGroup)` — 注册/更新 sender 的 JID 映射
- [x] `getJid(phone)` — 获取回复目标 JID
- [x] `getState(phone)` — 获取 sender 完整状态
- [x] `accumulate(phone, content)` — 累加响应文本
- [x] `getAccumulated(phone)` — 获取累积文本
- [x] `clearAccumulator(phone)` — 清除累加器
- [x] 替换 gateway.ts 中硬编码的 JID 构造逻辑

### 1.2 创建 PermissionManager 模块 _需求: FR-4_ ✅
- [x] 新建 `permission.ts`（9.3KB），实现权限请求状态机
- [x] `PermissionRequest` 接口：`tool_use_id`、`tool_name`、`description`、`expiresAt`
- [x] `requestPermission(sender, toolUseId, toolName, description)` — 格式化权限请求消息
- [x] `handleResponse(sender, text)` — 解析 yes/no 回复，调用 `permissionResponse` RPC
- [x] 60 秒超时自动 deny（setInterval 检查）
- [x] 与 SenderTracker 集成：每个 sender 独立的 pendingPermission 状态

### 1.3 创建 MediaHandler 模块 _需求: FR-5, FR-6_ ✅
- [x] 新建 `media.ts`（10KB），实现媒体下载/上传处理
- [x] `downloadMedia(sock, msg)` — 下载 WhatsApp 媒体到临时目录
- [x] `handleDocument(sock, msg, ipcClient)` — 文档消息处理（下载 → docUpload RPC → 返回文档 ID）
- [x] `handleImage(sock, msg)` — 图片消息处理（下载 → 保存为本地文件）
- [x] `sendFile(sock, jid, filePath, caption?)` — 发送文件（自动判断图片/文档）
- [x] 文件大小检查（上限 50MB）
- [x] 临时文件清理（处理完成后删除）

---

## Phase 2: 命令系统

### 2.1 命令框架 _需求: FR-1.1_ ✅
- [x] 新建 `commands.ts`（28.5KB），定义 `Command` 接口和 `CommandContext` 接口
- [x] 实现 `COMMAND_REGISTRY: Map<string, Command>`（24 个命令）
- [x] 实现 `parseCommand(text)` — 解析 `/command [args]` 格式
- [x] 实现 `isRegisteredCommand(name)` — 检查命令是否已注册
- [x] 实现 `dispatchCommand(ctx)` — 查表并调用 handler
- [x] 实现 `formatHelp()` — 生成帮助文本（列出所有命令及用法）

### 2.2 对话类命令 _需求: FR-1.2_ ✅
- [x] `/compact` → `compact` RPC → 格式化结果
- [x] `/think` → 作为普通消息发送 "think" 提示
- [x] `/model [name]` → `switchModel` RPC → 格式化当前/切换后模型
- [x] `/history [n]` → `talkTail` RPC → 格式化对话历史
- [x] `/search <query>` → `searchHistory` RPC → 格式化搜索结果
- [x] `/export` → `export` RPC → 发送导出文件为文档消息
- [x] `/abort` → `abort` RPC → 格式化确认

### 2.3 项目 & Git 命令 _需求: FR-1.3_ ✅
- [x] `/projects` → `projectsList` RPC → 格式化项目列表
- [x] `/git` → `gitStatus` RPC → 格式化 Git 状态
- [x] `/diff` → `gitDiff` RPC → 格式化 diff（代码块包裹）
- [x] `/commit [msg]` → `gitCommit` RPC → 格式化提交结果

### 2.4 工具 & 扩展命令 _需求: FR-1.4_ ✅
- [x] `/tools` → `listTools` RPC → 格式化工具列表
- [x] `/mcp` → `listMcpServers` RPC → 格式化 MCP 服务器列表
- [x] `/skills` → `listSkills` RPC → 格式化技能列表
- [x] `/plugins` → `listPlugins` RPC → 格式化插件列表

### 2.5 自动化命令 _需求: FR-1.5_ ✅
- [x] `/task <desc>` → `taskCreate` RPC → 格式化任务创建结果
- [x] `/tasks` → `taskList` RPC → 格式化任务列表
- [x] `/task_stop <id>` → `taskStop` RPC → 格式化停止结果
- [x] `/cron` → `cronList` RPC → 格式化定时任务列表

### 2.6 会话 & Spec 命令 _需求: FR-1.6, FR-1.7_ ✅
- [x] `/help` → 调用 `formatHelp()` 输出所有命令
- [x] `/status` → 组合 daemon info + WhatsApp 连接状态
- [x] `/start` → 欢迎消息 + 简要说明
- [x] `/clear` → 清除本地缓存
- [x] `/spec list` → `specList` RPC → 格式化
- [x] `/spec new <name>` → `specNew` RPC → 格式化
- [x] `/spec show <name>` → `specShow` RPC → 格式化
- [x] `/spec status <name>` → `specStatus` RPC → 格式化进度
- [x] `/spec run <name>` → `specRun` RPC → 格式化

---

## Phase 3: Gateway 主流程改造

### 3.1 共享会话 _需求: FR-2_ ✅
- [x] 修改 `daemon.ts` 的 `connect()` 方法，添加 `shared_session_id` 参数
- [x] `discoverAndConnect()` 传递 `sharedSessionId`
- [x] `gateway.ts` 的 `start()` 使用 `config.sharedSessionId`

### 3.2 Inbound 处理重构 _需求: FR-3, FR-4, FR-5_ ✅
- [x] 在 `setupInboundHandler` 中添加消息去重（`MessageQueue.isDuplicate(msgId)`）
- [x] 添加文档消息处理分支（`MediaHandler.handleDocument`）
- [x] 添加图片消息处理分支（`MediaHandler.handleImage`）
- [x] 添加权限回复检查（在命令检查之前）
- [x] 添加命令分发逻辑（以 `/` 开头 → `dispatchCommand`）
- [x] 注册 SenderTracker 映射（每次 inbound 都更新）

### 3.3 Outbound 处理重构 _需求: FR-4, FR-6_ ✅
- [x] 重构 `setupStreamHandler`，使用 SenderTracker 获取 JID
- [x] 添加 `permission_request` 事件处理（调用 `PermissionManager`）
- [x] 添加 `tool_result` 中的文件路径检测和媒体发送
- [x] 将 `responseAccumulators` 迁移到 SenderTracker + `processingFlags`

### 3.4 消息队列增强 _需求: FR-7.3_ ✅
- [x] 添加 `MAX_QUEUE_SIZE_PER_SENDER` 上限（configurable）
- [x] `enqueue()` 返回 boolean（false 表示队列满）
- [x] 队列满时回复用户提示信息
- [x] 添加消息 ID 去重缓存（LRU, 1000 条）

---

## Phase 4: Formatter & 配置增强

### 4.1 Formatter 增强 _需求: FR-1.3_ ✅
- [x] 添加表格 → 代码块等宽文本转换
- [x] 添加 `## heading` → `*bold heading*` 转换
- [x] 添加 `- [x] task` → `✅ task` / `☐ task` 转换
- [x] 添加 `[link](url)` → `url` 转换
- [x] 优化 `splitMessage` 分片策略（maxLength 4096 → 4000）

### 4.2 配置扩展 _需求: D-9_ ✅
- [x] 扩展 `WhatsAppConfig` 接口（添加 6 个新字段）
- [x] 更新 `loadWhatsAppConfig` 解析新字段
- [x] 更新 `DEFAULTS` 常量

---

## Phase 5: 健壮性 & 测试

### 5.1 Daemon 重连增强 _需求: FR-7.1_ ✅
- [x] 实现指数退避重连（5s → 10s → 30s → 60s，上限 config.reconnectMaxMs）
- [x] 重连成功后自动恢复 stream/event 订阅
- [x] 重连期间消息入队不丢弃

### 5.2 日志增强 _需求: FR-8_ ✅
- [x] `/status` 命令显示运行状态统计（通过 commands.ts）

### 5.3 单元测试
- [ ] `commands.test.ts` — 命令解析和格式化
- [ ] `senderTracker.test.ts` — JID 映射和状态管理
- [ ] `permission.test.ts` — 权限状态机
- [ ] `media.test.ts` — 媒体处理逻辑
- [ ] `formatter.test.ts` — 增强 formatter 测试覆盖

---

## 执行结果

| 指标 | 值 |
|------|-----|
| TypeScript 编译 | ✅ `tsc --noEmit` 通过 |
| 新建文件 | 4 个（senderTracker.ts, permission.ts, commands.ts, media.ts） |
| 修改文件 | 5 个（gateway.ts, config.ts, daemon.ts, formatter.ts, messageQueue.ts） |
| 新增代码量 | ~55KB（commands.ts 28.5KB + media.ts 10KB + permission.ts 9.3KB + senderTracker.ts 7.6KB） |
| 总代码量 | ~113KB（14 个 .ts 文件） |
| 命令注册数 | 24 个（含 /spec 5 个子命令） |
| 剩余任务 | 仅剩 5 个单元测试文件（Phase 5.3） |
