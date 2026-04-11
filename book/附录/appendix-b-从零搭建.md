# 附录 B：从零搭建 Agent Harness

## 最小可行 Agent

如果你想从零搭建一个类似 BaoClaw 的 Agent Harness，以下是最小实现路径：

### 第 1 步：ReAct 循环（1 天）

```python
# 最简单的 ReAct 循环
while True:
    response = llm.call(messages, tools)
    if response.has_tool_calls():
        for tool_call in response.tool_calls:
            result = execute_tool(tool_call)
            messages.append(tool_result(result))
    else:
        print(response.text)
        break
```

### 第 2 步：工具系统（1 天）

定义 Tool 接口，实现 BashTool 和 FileReadTool。

### 第 3 步：Daemon 化（2 天）

把 Agent 放到后台进程，通过 UDS + JSON-RPC 通信。

### 第 4 步：CLI 客户端（1 天）

连接 Daemon，发送消息，渲染流式输出。

### 第 5 步：记忆系统（1 天）

JSONL 文件存储记忆，注入系统提示词。

### 第 6 步：MCP 支持（2 天）

实现 MCP stdio transport，连接外部工具服务器。

### 第 7 步：多客户端（2 天）

添加 Telegram 网关，实现共享会话。

总计约 10 天，你就有了一个功能完整的 Agent Harness。

## 技术选型建议

| 组件 | 推荐 | 原因 |
|------|------|------|
| Daemon | Rust / Go | 性能、并发、内存安全 |
| CLI | TypeScript | 生态丰富、readline 支持好 |
| IPC | JSON-RPC + UDS | 简单、高效、无端口冲突 |
| 存储 | JSONL 文件 | 零依赖、易调试 |
| MCP | stdio transport | MCP 标准，生态丰富 |
