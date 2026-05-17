# 第 13 章：IPC 协议 —— JSON-RPC over UDS

## 13.1 协议选择

BaoClaw 使用 JSON-RPC 2.0 over Unix Domain Socket（UDS），NDJSON 分帧（每行一个 JSON 消息）。

为什么不用 HTTP/WebSocket？
- UDS 是本地通信，零网络开销
- 不需要端口管理，不会端口冲突
- 文件系统权限天然提供安全隔离

## 13.2 消息类型

### 请求（Request）
```json
{"jsonrpc":"2.0","id":1,"method":"submitMessage","params":{"prompt":"hello"}}
```

### 响应（Response）
```json
{"jsonrpc":"2.0","id":1,"result":{"status":"complete"}}
```

### 通知（Notification）—— 无 id，无需响应
```json
{"jsonrpc":"2.0","method":"stream/event","params":{"type":"assistant_chunk","content":"Hi"}}
```

## 13.3 方法列表（`claude-core/src/ipc/router.rs`）

```rust
pub enum ClientMethod {
    Initialize { cwd, model, settings, resume_session_id, shared_session_id },
    SubmitMessage { prompt },
    Abort,
    Shutdown,
    UpdateSettings { settings },
    PermissionResponse { tool_use_id, decision, rule },
    ListTools,
    ListMcpServers,
    ListSkills,
    ListPlugins,
    Compact,
    SwitchModel { model },
    GitDiff,
    GitCommit { message },
    GitStatus,
    MemoryList,
    MemoryAdd { content, category },
    MemoryDelete { id },
    MemoryClear,
    TaskCreate { description, prompt },
    TaskList,
    TaskStatus { task_id },
    TaskStop { task_id },
}
```

## 13.4 流式事件

Agent 的回复通过 `stream/event` 通知实时推送：

```
Client → submitMessage
Daemon → stream/event { type: "assistant_chunk", content: "你" }
Daemon → stream/event { type: "assistant_chunk", content: "好" }
Daemon → stream/event { type: "tool_use", tool_name: "Bash", ... }
Daemon → stream/event { type: "tool_result", output: "...", ... }
Daemon → stream/event { type: "result", status: "complete", ... }
Daemon → response { id: 1, result: { status: "complete" } }
```

客户端收到 `assistant_chunk` 就实时渲染，收到 `result` 就知道本轮结束。

## 13.5 小结

JSON-RPC 2.0 + UDS + NDJSON 是一个简单高效的本地 IPC 方案。请求-响应模式处理命令，通知模式处理流式事件。
