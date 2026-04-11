# 第 5 章：MCP 协议 —— 让 Agent 获得无限能力

## 5.1 问题：内置工具的局限

内置工具是有限的。你不可能为每个场景都写一个 Rust 工具。用户可能需要：
- 控制桌面（截图、点击）
- 操作数据库
- 调用企业内部 API
- 连接智能家居设备

MCP（Model Context Protocol）解决了这个问题：它定义了一个标准协议，让任何外部程序都能作为工具服务器接入 Agent。

## 5.2 MCP 协议概述

MCP 使用 JSON-RPC 2.0 over stdio 通信：

```
Agent (Client)                    MCP Server (外部进程)
    │                                    │
    │── initialize ──────────────────────>│
    │<─ capabilities, serverInfo ─────────│
    │── notifications/initialized ───────>│
    │── tools/list ──────────────────────>│
    │<─ { tools: [...] } ────────────────│
    │                                    │
    │── tools/call { name, arguments } ──>│
    │<─ { content: [...] } ──────────────│
```

## 5.3 BaoClaw 的 MCP 实现

### 传输层（`claude-core/src/mcp/transport.rs`）

`StdioTransport` 负责 spawn 子进程并通过 stdin/stdout 通信：

```rust
pub async fn spawn(command: &str, args: &[String], env: &HashMap<String, String>) 
    -> Result<Self, McpError> 
{
    let mut child = Command::new(command).args(args).envs(env)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn()?;
    
    // MCP initialize 握手
    transport.request("initialize", Some(json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": { "name": "baoclaw", "version": "0.3.0" }
    }))).await?;
    
    // 发送 initialized 通知
    transport.notify("notifications/initialized", None).await?;
    
    Ok(transport)
}
```

### 工具发现（`claude-core/src/mcp/client.rs`）

连接成功后，调用 `tools/list` 获取工具列表：

```rust
pub async fn refresh_tools(&self) -> Result<(), McpError> {
    let result = transport.request("tools/list", None).await?;
    let tool_defs: Vec<McpToolDef> = serde_json::from_value(result["tools"].clone())?;
    *self.tools.write().await = tool_defs;
}
```

注意：MCP 返回的字段名是 camelCase（`inputSchema`），而 Rust 用 snake_case。需要 `#[serde(rename = "inputSchema")]`。

### 工具包装（`claude-core/src/mcp/tool_wrapper.rs`）

`McpToolWrapper` 将 MCP 工具包装为 BaoClaw 的 `Tool` trait：

```rust
impl Tool for McpToolWrapper {
    fn name(&self) -> &str { &self.tool_def.name }
    fn prompt(&self) -> String { 
        format!("[MCP:{}] {}", self.server_name, self.tool_def.description) 
    }
    fn max_result_size_chars(&self) -> usize { 10_000_000 } // 10MB，支持截图
    
    async fn call(&self, input: Value, ...) -> Result<ToolResult, ToolError> {
        self.client.call_tool(&self.tool_def.name, input).await
    }
}
```

## 5.4 配置

MCP 服务器在 `~/.baoclaw/mcp.json` 中配置：

```json
{
    "mcpServers": {
        "computer-control-mcp": {
            "command": "uvx",
            "args": ["computer-control-mcp@latest"]
        }
    }
}
```

BaoClaw 在 daemon 启动时自动发现、连接所有配置的 MCP 服务器，并将它们的工具注册到 `engine_tools` 中。

## 5.5 踩过的坑

1. **字段名不匹配** —— MCP 用 `inputSchema`（camelCase），Rust struct 默认 snake_case，导致所有工具解析失败返回 0 个。解决：`#[serde(rename)]`。
2. **结果截断** —— 截图工具返回的 base64 数据超过 100K 字符被截断，JSON 不完整。解决：MCP 工具覆盖 `max_result_size_chars` 为 10MB。
3. **base64 图片在对话历史中** —— 一张 2560×1600 截图的 base64 约 5MB，直接撑爆上下文窗口。解决：`strip_base64_images` 在存入对话历史前移除图片数据。

## 5.6 小结

MCP 是 Agent 能力扩展的关键协议。通过标准化的 JSON-RPC 接口，任何外部程序都能成为 Agent 的工具。BaoClaw 通过 `McpToolWrapper` 将 MCP 工具无缝集成到内置工具系统中。
