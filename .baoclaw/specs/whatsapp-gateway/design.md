# WhatsApp Gateway — 设计文档

## 架构总览

```
baoclaw-whatsapp/src/
├── gateway.ts          # 主进程：启动、消息分发、stream 处理（增强）
├── commands.ts         # 斜杠命令注册表 + 格式化函数（新建）
├── session.ts          # Baileys 会话管理（微调）
├── config.ts           # 配置加载（已有）
├── daemon.ts           # Daemon 发现（已有）
├── ipcClient.ts        # JSON-RPC 客户端（已有）
├── allowlist.ts        # 白名单过滤（已有）
├── rateLimiter.ts      # 速率限制（已有）
├── messageQueue.ts     # 消息队列（增强：上限+去重）
├── formatter.ts        # Markdown→WhatsApp 格式转换（增强）
├── media.ts            # 媒体下载/上传处理（新建）
├── permission.ts       # 权限请求状态机（新建）
├── senderTracker.ts    # JID 映射 + per-sender 状态（新建）
└── types.d.ts          # 类型声明（增强）
```

---

## D-1: 命令系统设计

### D-1.1: 命令注册表

```typescript
// commands.ts
interface Command {
  name: string;           // 命令名，如 "tools"
  description: string;    // 帮助描述
  usage?: string;         // 用法示例
  handler: (ctx: CommandContext) => Promise<string | void>;
}

interface CommandContext {
  ipcClient: IpcClient;
  args: string;           // 命令参数
  sender: string;         // 发送者电话号码
  jid: string;            // WhatsApp JID
  sock: any;              // Baileys socket
}

const COMMAND_REGISTRY: Map<string, Command> = new Map();
```

### D-1.2: 命令处理流程

```
收到消息 → 以 / 开头？
  ├─ 是 → parseCommand() → 查注册表
  │       ├─ 已注册 → handler() → 格式化结果 → 发回 WhatsApp
  │       └─ 未注册 → "未知命令，发送 /help 查看帮助"
  └─ 否 → 消息队列 → submitMessage → stream/event 流处理
```

### D-1.3: 命令格式化原则

- WhatsApp 格式限制：支持 *bold*、_italic_、```code```、~strikethrough~
- 表格用等宽文本模拟（代码块包裹）
- 列表用 WhatsApp 有序/无序列表
- 截断超长输出（单条消息上限 65536 字符，实际控制在 4000 字符以内便于手机阅读）

### D-1.4: 命令实现策略

**共享模式**：命令 handler 调用 RPC → 获得结果 → 用 `format*()` 函数格式化为 WhatsApp 文本。

每条命令的 `format*()` 函数独立实现，不与 Telegram 共享（两端格式差异太大），但 RPC 调用逻辑完全一致。

---

## D-2: JID 映射与 Per-Sender 状态

### D-2.1: SenderTracker

```typescript
// senderTracker.ts
interface SenderState {
  jid: string;                          // 回复目标 JID
  isGroup: boolean;                     // 是否群聊
  responseAccumulator: string;          // 响应文本累加
  pendingPermission: PermissionRequest | null;  // 待处理权限请求
  messageCount: number;                 // 已处理消息计数
}

class SenderTracker {
  private senders = new Map<string, SenderState>();  // phone → state

  // 注册/更新 sender 的 JID
  registerSender(phone: string, jid: string, isGroup: boolean): void;

  // 获取 sender 的 JID（用于回复）
  getJid(phone: string): string | null;

  // 获取 sender 状态
  getState(phone: string): SenderState | undefined;

  // 清除 sender 的响应累加器
  clearAccumulator(phone: string): void;
}
```

### D-2.2: 流程改造

**Inbound**（收消息时）：
```typescript
// 注册 sender → jid 映射
senderTracker.registerSender(senderPhone, jid, isGroup);
```

**Outbound**（发消息时）：
```typescript
// 从 tracker 获取正确的 jid，不再硬编码
const state = senderTracker.getState(sender);
const jid = state?.jid ?? fallbackJid;
```

---

## D-3: 权限交互设计

### D-3.1: 状态机

```
                    permission_request 事件
                           │
                           ▼
               ┌──── PENDING_PERMISSION ────┐
               │  显示权限请求消息给用户      │
               │  设置 60s 超时定时器        │
               │                            │
          用户回复 yes/deny           超时 60s
               │                            │
               ▼                            ▼
     permissionResponse          permissionResponse
     (decision: allow)          (decision: deny)
               │                            │
               └──────────── ───────────────┘
                            │
                            ▼
                      恢复正常状态
```

### D-3.2: 消息格式

权限请求消息：
```
🔐 *权限请求*
工具: read_file
描述: 读取 /home/user/project/src/main.rs

请回复 *yes* 允许 或 *no* 拒绝
（60秒后自动拒绝）
```

### D-3.3: 状态隔离

- 每个 sender 独立维护 `pendingPermission` 状态
- 同时只有一个权限请求待处理
- 新的权限请求在旧的未完成时自动 deny 旧的

---

## D-4: 文档/媒体处理设计

### D-4.1: 媒体下载

```typescript
// media.ts
class MediaHandler {
  // 下载 WhatsApp 媒体文件到临时目录
  async downloadMedia(sock: any, msg: any): Promise<MediaFile | null>;

  // 处理文档消息（PDF/DOCX）
  async handleDocument(sock: any, msg: any, ipcClient: IpcClient): Promise<string | null>;

  // 处理图片消息
  async handleImage(sock: any, msg: any): Promise<string | null>;
}

interface MediaFile {
  path: string;          // 本地文件路径
  mimeType: string;      // MIME 类型
  fileName: string;      // 原始文件名
  size: number;          // 文件大小（字节）
}
```

### D-4.2: 文档处理流程

```
收到文档消息
    │
    ▼
下载到 /tmp/baoclaw-whatsapp-{uuid}/
    │
    ▼
调用 docUpload RPC → 获得文档 ID
    │
    ▼
将文档 ID 作为 attachment 传给 submitMessage
    │
    ▼
清理临时文件
```

### D-4.3: 媒体发送

当 daemon 响应中包含文件路径时（通过 `tool_result` 或 `result` 中的特殊标记）：
- 图片文件（.png/.jpg/.webp）→ WhatsApp 图片消息
- 其他文件 → WhatsApp 文档消息

检测逻辑：解析 `tool_result.output` 中是否包含已知文件路径模式。

---

## D-5: 消息队列增强

### D-5.1: 上限控制

```typescript
// messageQueue.ts 增强
const MAX_QUEUE_SIZE_PER_SENDER = 100;

enqueue(sender: string, text: string): boolean {
  if (this.getQueueSize(sender) >= MAX_QUEUE_SIZE_PER_SENDER) {
    return false;  // 拒绝入队
  }
  // ... 原有逻辑
}
```

### D-5.2: 消息去重

```typescript
// messageQueue.ts 新增
const MSG_ID_CACHE_SIZE = 1000;

private seenMsgIds = new Map<string, number>();  // msgId → timestamp

isDuplicate(msgId: string): boolean {
  if (this.seenMsgIds.has(msgId)) return true;
  this.seenMsgIds.set(msgId, Date.now());
  // LRU 清理
  if (this.seenMsgIds.size > MSG_ID_CACHE_SIZE) {
    const oldest = [...this.seenMsgIds.entries()]
      .sort((a, b) => a[1] - b[1])
      .slice(0, 100);
    for (const [id] of oldest) this.seenMsgIds.delete(id);
  }
  return false;
}
```

---

## D-6: 共享会话设计

### D-6.1: 初始化参数

```typescript
// daemon.ts connect() 方法增强
async connect(info: DaemonInfo): Promise<IpcClient> {
  const client = new IpcClient();
  await client.connect(info.socket);
  await client.request('initialize', {
    cwd: info.cwd,
    shared_session_id: 'whatsapp',  // 新增
  });
  return client;
}
```

### D-6.2: 多端协调

- `shared_session_id: 'whatsapp'` 让 daemon 知道这是 WhatsApp 网关
- daemon 按 session_id 隔离不同网关的 stream/event 推送
- WhatsApp 和 Telegram 可以同时连接同一个 daemon，各自收到各自的响应

---

## D-7: Formatter 增强

### D-7.1: 额外转换

当前 `formatter.ts` 只处理 bold/italic/code。增强后处理：

| Markdown | WhatsApp |
|----------|----------|
| `\| table \|` | ```代码块包裹的等宽文本``` |
| `## heading` | *bold heading* |
| `- [x] task` | ✅ task / ☐ task |
| `> quote` | > quote（WhatsApp 原生支持） |
| `[link](url)` | url（WhatsApp 不支持内联链接） |

### D-7.2: 长消息分片

当前 `splitMessage` 按 4096 分片。改为：
- 首选在 `\n\n`（段落边界）处分割
- 次选在 `\n`（行边界）处分割
- 最后按 4000 字符硬切（预留安全余量）

---

## D-8: Daemon 重连增强

### D-8.1: 指数退避

```typescript
const RECONNECT_BASE_MS = 5_000;
const RECONNECT_MAX_MS = 300_000;  // 5 分钟

private async reconnectDaemon(sock: any, attempt: number = 1): Promise<void> {
  const delay = Math.min(
    RECONNECT_BASE_MS * Math.pow(2, attempt - 1),
    RECONNECT_MAX_MS
  );
  console.warn(`Reconnect attempt ${attempt} in ${delay}ms...`);
  await sleep(delay);

  try {
    const { client, info } = await this.daemonConnector.discoverAndConnect();
    // ... 连接成功，重置状态
  } catch {
    this.reconnectDaemon(sock, attempt + 1);
  }
}
```

### D-8.2: 重连期间消息处理

- 重连期间 inbound 消息正常入队（队列上限 100 条/人）
- 队列满时回复用户："Daemon 连接中，请稍后重试"
- 重连成功后自动恢复队列处理

---

## D-9: 配置扩展

### D-9.1: config.json whatsapp 段扩展

```json
{
  "whatsapp": {
    "enabled": true,
    "phoneNumber": "+8613812345678",
    "allowFrom": ["+8613812345678", "+8613987654321"],
    "dmPolicy": "allow",
    "groupPolicy": "ignore",
    "maxQueueSize": 100,
    "permissionTimeoutMs": 60000,
    "reconnectMaxMs": 300000,
    "sharedSessionId": "whatsapp",
    "mediaEnabled": true,
    "mediaMaxSizeMb": 50
  }
}
```

### D-9.2: 新增字段默认值

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `maxQueueSize` | 100 | 每人消息队列上限 |
| `permissionTimeoutMs` | 60000 | 权限请求超时 |
| `reconnectMaxMs` | 300000 | 最大重连等待 |
| `sharedSessionId` | "whatsapp" | 共享会话 ID |
| `mediaEnabled` | true | 是否启用媒体处理 |
| `mediaMaxSizeMb` | 50 | 最大媒体文件大小 |

---

## D-10: gateway.ts 主流程改造

### D-10.1: 启动流程

```
loadConfig()
  → validateConfig()
  → SessionManager.initialize()
  → SenderTracker 初始化
  → DaemonConnector.discoverAndConnect({ shared_session_id: 'whatsapp' })
  → registerCommands()
  → setupStreamHandler()
  → setupInboundHandler()
  → writePidFile()
  → setupShutdownHandlers()
```

### D-10.2: Inbound 分发

```
messages.upsert 事件
  │
  ├─ 去重检查（MessageTracker.isDuplicate）
  ├─ 非文本/文档/图片 → 忽略
  ├─ 自己的消息 → 忽略
  ├─ 广播消息 → 忽略
  │
  ├─ 文档消息 → MediaHandler.handleDocument()
  ├─ 图片消息 → MediaHandler.handleImage()
  │
  ├─ 文本消息:
  │   ├─ 权限回复检查（PermissionManager.handleResponse）
  │   ├─ 命令检查（以 / 开头 → dispatchCommand）
  │   └─ 普通消息 → messageQueue → processQueue → submitMessage
```

### D-10.3: Outbound 流处理

```
stream/event 通知
  │
  ├─ assistant_chunk → SenderTracker.accumulate(sender, content)
  ├─ tool_use → 发送 "🔧 Using: {toolName}" 状态消息
  ├─ tool_result → 错误时发送错误消息；检查是否包含文件路径
  │                 ├─ 图片路径 → WhatsApp 图片消息
  │                 └─ 文件路径 → WhatsApp 文档消息
  ├─ permission_request → PermissionManager.request()
  ├─ result → 格式化累积文本 → 分片发送 → 清除累加器
  └─ error → 发送错误消息 → 清除累加器
```

---

## 模块依赖关系

```
gateway.ts
  ├── commands.ts ──────── ipcClient (RPC)
  ├── session.ts ──────── @whiskeysockets/baileys
  ├── daemon.ts ────────── ipcClient
  ├── ipcClient.ts ─────── net (UDS)
  ├── senderTracker.ts ─── (纯内存状态)
  ├── permission.ts ────── senderTracker + ipcClient
  ├── media.ts ─────────── baileys (下载) + ipcClient (上传)
  ├── formatter.ts ─────── (纯函数)
  ├── messageQueue.ts ──── (纯内存队列)
  ├── rateLimiter.ts ───── (纯内存限流)
  ├── allowlist.ts ─────── (纯函数)
  └── config.ts ────────── fs (文件读取)
```
