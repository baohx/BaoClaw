# IPC 与多客户端源码分析

> 本文档为 book2 第四章 "IPC 与多客户端" 提供源码参考材料。
> 基于实际代码分析，涵盖 Requirements 9.1, 9.2。

## 概述

BaoClaw 的 IPC 架构采用 **JSON-RPC 2.0 over Unix Domain Socket** 设计，支持多客户端同时连接同一守护进程。核心组件包括：

1. **Rust IPC 服务端** (`baoclaw-core/src/ipc/`) - 守护进程端
2. **TypeScript IPC 客户端** (`ts-ipc/`) - 终端 CLI
3. **网关客户端** (`baoclaw-whatsapp/`, `baoclaw-telegram/`, `baoclaw-feishu/`) - 多渠道接入

---

## 1. IPC 协议层 (Rust)

### 1.1 协议定义

**文件路径:** `baoclaw-core/src/ipc/protocol.rs`

JSON-RPC 2.0 消息类型定义：

```rust
// 请求 ID - 支持数字或字符串
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

// JSON-RPC 2.0 请求
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,           // "2.0"
    pub method: String,            // RPC 方法名
    #[serde(default)]
    pub params: Value,             // 参数（可选）
    pub id: RequestId,             // 请求 ID
}

// JSON-RPC 2.0 成功响应
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Value,             // 结果
    pub id: RequestId,
}
```

**设计要点：**
- `RequestId` 使用 `#[serde(untagged)]` 支持 JSON-RPC 规范中的数字或字符串 ID
- 错误响应的 `id` 可为 `None`（如解析错误时无法确定请求 ID）

### 1.2 统一消息类型

```rust
#[derive(Clone, Debug)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    ErrorResponse(JsonRpcErrorResponse),
    Notification(JsonRpcNotification),
}
```

自定义反序列化逻辑，根据 JSON 字段自动判断消息类型：
- 有 `error` 字段 → ErrorResponse
- 有 `result` 字段 → Response
- 有 `method` 且有 `id` → Request
- 有 `method` 且无 `id` → Notification

### 1.3 NDJSON 帧格式

```rust
// 编码：JSON + 换行符
pub fn encode_ndjson(message: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    Ok(bytes)
}

// 解码：单行 JSON → JsonRpcMessage
pub fn decode_ndjson_line(line: &str) -> Result<JsonRpcMessage, serde_json::Error> {
    let trimmed = line.trim();
    serde_json::from_str(trimmed)
}
```

NDJSON (Newline-Delimited JSON) 格式的优势：
- 流式处理友好，无需预先知道消息长度
- 天然支持消息边界
- 便于调试（每行一个完整 JSON）

### 1.4 IPC 服务端

**文件路径:** `baoclaw-core/src/ipc/server.rs`

```rust
/// IPC 服务端 - 监听 Unix Domain Socket
pub struct IpcServer {
    listener: UnixListener,
    socket_path: PathBuf,
}

/// IPC 连接 - 带缓冲的读写
pub struct IpcConnection {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: BufWriter<tokio::net::unix::OwnedWriteHalf>,
}
```

**安全特性：**
- 自动清理旧 socket 文件，避免 "address already in use"
- 设置 0600 权限，防止其他用户连接
- 析构时自动清理 socket 文件

### 1.5 路由层

**文件路径:** `baoclaw-core/src/ipc/router.rs`

定义客户端可调用的 RPC 方法：

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params")]
pub enum ClientMethod {
    #[serde(rename = "initialize")]
    Initialize {
        cwd: PathBuf,
        model: Option<String>,
        settings: Value,
        #[serde(default)]
        resume_session_id: Option<String>,
        #[serde(default)]
        shared_session_id: Option<String>,
    },

    #[serde(rename = "submitMessage")]
    SubmitMessage {
        prompt: Value,
        uuid: Option<String>,
        #[serde(default)]
        attachments: Option<Vec<Value>>,
    },

    #[serde(rename = "permissionResponse")]
    PermissionResponse {
        tool_use_id: String,
        decision: String,
        rule: Option<String>,
    },

    #[serde(rename = "abort")]
    Abort,

    // ... 更多方法：listTools, listMcpServers, compact, switchModel 等
}
```

使用 `#[serde(tag = "method", content = "params")]` 实现方法名到参数类型的映射。

### 1.6 事件层

**文件路径:** `baoclaw-core/src/ipc/events.rs`

```rust
/// 引擎事件 → stream/event 通知
pub fn engine_event_to_notification(event: &EngineEvent) -> JsonRpcNotification {
    JsonRpcNotification::new(
        "stream/event",
        serde_json::to_value(event).unwrap_or(Value::Null),
    )
}
```

**流事件类型：**
- `AssistantChunk` - AI 输出文本块
- `ThinkingChunk` - 思考过程块
- `ToolUse` - 工具调用
- `ToolResult` - 工具结果
- `PermissionRequest` - 权限请求
- `Result` - 查询完成
- `Error` - 错误

---

## 2. IPC 客户端层 (TypeScript)

### 2.1 核心 IPC 客户端

**文件路径:** `ts-ipc/client.ts`

```typescript
export class IpcClient {
  private socket: net.Socket | null = null;
  private buffer = '';
  private nextId = 1;
  private pendingRequests = new Map<number | string, {
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
    timer: ReturnType<typeof setTimeout>;
  }>();
  private notificationHandlers = new Map<string, NotificationHandler[]>();
}
```

**请求-响应机制：**
- 使用 Map 存储 pending 请求
- 每个请求设置超时（默认 30s）
- 支持 Promise-based API

**NDJSON 帧处理：**
- 缓冲接收数据
- 按换行符分割
- 解析单行 JSON
- 根据 id/method 字段路由

**通知订阅：**
- `onNotification(method, handler)` 注册处理器
- 返回取消订阅函数
- 支持多个处理器

### 2.2 流事件处理器

**文件路径:** `ts-ipc/streamHandler.ts`

```typescript
export interface StreamHandlerManager {
  onStreamEvent(handler: StreamEventHandler): () => void;
  onStatePatch(handler: StatePatchHandler): () => void;
  onEventType<T extends StreamEvent['type']>(
    type: T,
    handler: (event: Extract<StreamEvent, { type: T }>) => void,
  ): () => void;
  dispose(): void;
}
```

**状态补丁应用：** 使用 JSON Pointer (RFC 6901) 路径定位和修改状态对象。

### 2.3 CLI 终端客户端

**文件路径:** `ts-ipc/cli.ts`

**守护进程发现：**
```typescript
function discoverDaemons(): DaemonInfo[] {
  const dir = path.join(os.tmpdir(), 'baoclaw-sockets');
  // 扫描 .json 文件，检查进程存活和 socket 存在
}
```

**守护进程启动：**
- 使用 `detached: true` 启动子进程
- 从 stdout 读取 socket 路径（`SOCKET:<path>` 格式）
- 设置 60s 超时

**初始化流程：**
1. 发现/启动守护进程
2. 连接 Unix Domain Socket
3. 发送 `initialize` 请求（含 `shared_session_id`）
4. 订阅 `stream/event` 通知
5. 进入 REPL 循环

---

## 3. 网关实现（多客户端）

### 3.1 统一 IPC 客户端模式

所有网关使用相同的 IPC 客户端实现模式：

| 网关 | 文件路径 | 特点 |
|------|----------|------|
| WhatsApp | `baoclaw-whatsapp/src/ipcClient.ts` | 独立模块 |
| Telegram | `baoclaw-telegram/src/gateway.ts` | 内联类 |
| Feishu | `baoclaw-feishu/src/ipcClient.ts` | 独立模块 |

### 3.2 WhatsApp 网关架构

**文件路径:** `baoclaw-whatsapp/src/gateway.ts`

**核心组件：**
- `SessionManager` - WhatsApp 连接（Baileys）
- `DaemonConnector` - 守护进程发现
- `IpcClient` - IPC 客户端
- `SenderTracker` - 发送者追踪
- `PermissionManager` - 权限管理
- `MediaHandler` - 媒体处理
- `MessageQueue` - 消息队列

**入站消息处理流程：**
1. 去重检查
2. 允许列表检查
3. 速率限制
4. 命令解析（以 `/` 开头）
5. 消息入队
6. 调用 `submitMessage` RPC

### 3.3 Telegram 网关架构

**文件路径:** `baoclaw-telegram/src/gateway.ts`

**技术栈：** node-telegram-bot-api

**消息处理：**
- 使用 `polling` 模式接收消息
- 支持 Markdown → Telegram HTML 转换
- 自动提取和发送 base64 图片

### 3.4 Feishu 网关架构

**文件路径:** `baoclaw-feishu/src/gateway.ts`

**架构特点：**
- 使用 `lark-cli` 命令行工具
- 事件流：`lark-cli event consume` (NDJSON stdout)
- 消息发送：`lark-cli messages-send`

```
lark-cli event consume → Gateway → Unix Socket → Daemon
Daemon stream/event → Gateway → lark-cli messages-send
```

---

## 4. 关键设计模式

### 4.1 会话共享（Shared Session）

多个客户端可以通过 `shared_session_id` 共享同一个会话：

```typescript
// CLI 连接
await client.request('initialize', {
  cwd: process.cwd(),
  shared_session_id: 'default'
});

// WhatsApp 网关连接
await client.request('initialize', {
  cwd: info.cwd,
  shared_session_id: config.sharedSessionId
});
```

**用途：** 不同渠道的用户可以共享同一对话历史。

### 4.2 消息队列（Per-Client Queue）

每个网关实现自己的消息队列，确保：
- 每个发送者一次只处理一条消息
- 按顺序处理，不乱序
- 支持队列满时的背压

```typescript
class MessageQueue {
  private queues = new Map<string, string[]>();
  private processing = new Set<string>();

  enqueue(sender: string, text: string, maxSize: number): boolean;
  dequeue(sender: string): string | undefined;
  isProcessing(sender: string): boolean;
}
```

### 4.3 发送者追踪（Sender Tracking）

WhatsApp 网关使用 `SenderTracker` 管理发送者状态：

```typescript
class SenderTracker {
  // 发送者 → JID 映射
  private senderToJid = new Map<string, string>();
  // 响应累积器
  private accumulators = new Map<string, string>();

  registerSender(phone: string, jid: string, isGroup: boolean): void;
  getJid(phone: string): string | null;
  accumulate(phone: string, content: string): void;
  getAccumulated(phone: string): string;
  clearAccumulator(phone: string): void;
}
```

**用途：** 在异步流处理中追踪当前活跃的发送者。

### 4.4 守护进程发现机制

**元数据文件：** `/tmp/baoclaw-sockets/<session>.json`

```typescript
interface DaemonInfo {
  pid: number;          // 进程 ID
  cwd: string;          // 工作目录
  session_id: string;   // 会话 ID
  socket: string;       // Socket 路径
  started_at: string;   // 启动时间 (ISO 8601)
}
```

**发现流程：**
1. 扫描 `/tmp/baoclaw-sockets/*.json`
2. 检查进程存活（`process.kill(pid, 0)`）
3. 检查 socket 文件存在
4. 选择最新启动的守护进程

### 4.5 优雅关闭

所有网关实现统一的关闭流程：

```typescript
process.on('SIGTERM', () => shutdown('SIGTERM'));
process.on('SIGINT', () => shutdown('SIGINT'));

async function shutdown(signal: string) {
  // 1. 设置超时强制退出
  const forceTimer = setTimeout(() => process.exit(1), 10000);

  // 2. 保存会话状态
  await session.disconnect();

  // 3. 关闭 IPC 连接
  await ipcClient.disconnect();

  // 4. 清理 PID 文件
  removePidFile();

  clearTimeout(forceTimer);
  process.exit(0);
}
```

---

## 5. 架构图

### 5.1 整体架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                        BaoClaw Daemon                                │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────────────────┐   │
│  │ IpcServer   │→  │ Router      │→  │ QueryEngine             │   │
│  │ (UDS)       │   │             │   │                         │   │
│  └─────────────┘   └─────────────┘   └─────────────────────────┘   │
│         ↑                                     │                      │
│         │                                     ▼                      │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────────────────┐   │
│  │ Events      │←  │ ToolExecutor│←  │ Stream Events           │   │
│  │             │   │             │   │ (assistant_chunk, etc.) │   │
│  └─────────────┘   └─────────────┘   └─────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
         │
         │ Unix Domain Socket (NDJSON)
         ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        IPC Clients                                   │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────────────────┐   │
│  │ CLI Client  │   │ WhatsApp    │   │ Telegram / Feishu       │   │
│  │ (ts-ipc)    │   │ Gateway     │   │ Gateway                 │   │
│  └─────────────┘   └─────────────┘   └─────────────────────────┘   │
│         │                 │                     │                   │
│         ▼                 ▼                     ▼                   │
│    Terminal          WhatsApp API          Telegram API / Feishu   │
└─────────────────────────────────────────────────────────────────────┘
```

### 5.2 消息流程

```
客户端                    守护进程                    AI 引擎
  │                         │                          │
  │  initialize {cwd, ...}  │                          │
  │ ─────────────────────►  │                          │
  │                         │  create QueryEngine      │
  │                         │ ───────────────────────► │
  │  {session_id, ...}      │                          │
  │ ◄─────────────────────  │                          │
  │                         │                          │
  │  submitMessage {prompt} │                          │
  │ ─────────────────────►  │                          │
  │                         │  process query           │
  │                         │ ───────────────────────► │
  │                         │                          │
  │  notification:          │                          │
  │  stream/event           │                          │
  │  {type: assistant_chunk}│  ◄────────────────────── │
  │ ◄─────────────────────  │                          │
  │                         │                          │
  │  notification:          │                          │
  │  stream/event           │                          │
  │  {type: result}         │  ◄────────────────────── │
  │ ◄─────────────────────  │                          │
```

---

## 6. 源文件索引

### Rust 核心实现

| 文件 | 行数 | 功能 |
|------|------|------|
| `baoclaw-core/src/ipc/mod.rs` | 6 | 模块导出 |
| `baoclaw-core/src/ipc/protocol.rs` | ~450 | JSON-RPC 协议定义 |
| `baoclaw-core/src/ipc/server.rs` | ~280 | IPC 服务端实现 |
| `baoclaw-core/src/ipc/router.rs` | ~450 | RPC 路由定义 |
| `baoclaw-core/src/ipc/events.rs` | ~200 | 事件转换 |

### TypeScript 客户端实现

| 文件 | 行数 | 功能 |
|------|------|------|
| `ts-ipc/client.ts` | ~180 | 核心 IPC 客户端 |
| `ts-ipc/types.ts` | ~40 | 类型定义 |
| `ts-ipc/streamHandler.ts` | ~130 | 流事件处理 |
| `ts-ipc/cli.ts` | ~2700 | 终端 CLI |

### 网关实现

| 文件 | 行数 | 功能 |
|------|------|------|
| `baoclaw-whatsapp/src/gateway.ts` | ~400 | WhatsApp 网关主逻辑 |
| `baoclaw-whatsapp/src/ipcClient.ts` | ~100 | IPC 客户端 |
| `baoclaw-whatsapp/src/daemon.ts` | ~100 | 守护进程发现 |
| `baoclaw-telegram/src/gateway.ts` | ~1300 | Telegram 网关（含内联 IPC） |
| `baoclaw-feishu/src/gateway.ts` | ~350 | Feishu 网关 |
| `baoclaw-feishu/src/ipcClient.ts` | ~100 | IPC 客户端 |

---

## 7. 扩展阅读

- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification)
- [Unix Domain Sockets](https://man7.org/linux/man-pages/man7/unix.7.html)
- [NDJSON Format](http://ndjson.org/)
- [JSON Pointer (RFC 6901)](https://datatracker.ietf.org/doc/html/rfc6901)
