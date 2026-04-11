# 第 2 章：ReAct 循环 —— Agent 的心跳

## 2.1 什么是 ReAct

ReAct（Reasoning + Acting）是 Agent 的核心运行模式。它的本质是一个循环：

```
用户消息 → LLM 思考 → 决定行动 → 执行工具 → 观察结果 → LLM 再思考 → ... → 最终回复
```

这个循环是 Agent 的"心跳"。没有它，LLM 只能做一次性的问答；有了它，Agent 可以完成任意复杂的多步骤任务。

## 2.2 BaoClaw 的 ReAct 实现

在 BaoClaw 中，ReAct 循环实现在 `run_query_loop` 函数中（`claude-core/src/engine/query_engine.rs`）。

核心流程：

```rust
loop {
    // 1. 构建 API 请求（包含完整对话历史 + 工具定义）
    let request = build_api_request(&messages, &config);
    
    // 2. 调用 LLM API（流式）
    let mut stream = api_client.create_message_stream(request).await?;
    
    // 3. 处理流式响应，收集 content blocks
    //    - TextBlock: AI 的文字回复
    //    - ToolUseBlock: AI 决定调用的工具
    //    - ThinkingBlock: 扩展思考内容
    while let Some(event) = stream.next().await { ... }
    
    // 4. 检查是否有工具调用
    let tool_uses = extract_tool_uses(&assistant_content_blocks);
    
    if tool_uses.is_empty() {
        // 没有工具调用 → 任务完成，返回结果
        return;
    }
    
    // 5. 执行工具
    let tool_results = execute_tools(&tools, &tool_uses, &context).await;
    
    // 6. 将工具结果加入对话历史
    messages.push(build_tool_result_message(&tool_results));
    
    // 7. 回到步骤 1，让 LLM 看到工具结果后继续思考
    turn_count += 1;
}
```

关键设计点：

### 2.2.1 流式处理

BaoClaw 使用 SSE（Server-Sent Events）流式接收 LLM 的响应。每收到一个 token，就通过事件通道发送给客户端，实现实时打字效果：

```rust
ApiStreamEvent::ContentBlockDelta { delta, .. } => {
    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
        // 实时发送给客户端
        tx.send(EngineEvent::AssistantChunk { content: text.to_string() }).await;
    }
}
```

### 2.2.2 工具调用的闭环

当 LLM 决定调用工具时，它会在响应中包含一个 `tool_use` content block：

```json
{
    "type": "tool_use",
    "id": "tu_001",
    "name": "Bash",
    "input": { "command": "ls -la" }
}
```

BaoClaw 提取这些 tool_use blocks，执行对应的工具，然后将结果作为 `tool_result` 消息追加到对话历史中。LLM 在下一轮看到工具结果后，可以继续调用更多工具或给出最终回复。

### 2.2.3 终止条件

循环在以下情况终止：

1. **LLM 不再调用工具** —— `tool_uses.is_empty()`，任务完成
2. **达到最大轮次** —— `turn_count >= max_turns`，防止无限循环
3. **用户中止** —— `abort` 信号被触发
4. **API 错误** —— 网络错误、认证失败等
5. **上下文窗口超出** —— 自动 compact 后重试

## 2.3 事件驱动的架构

BaoClaw 的 ReAct 循环不是简单的请求-响应，而是事件驱动的。`QueryEngine` 通过 `mpsc::channel` 向客户端发送各种事件：

```rust
pub enum EngineEvent {
    AssistantChunk { content: String },     // AI 输出的文字片段
    ThinkingChunk { content: String },      // 扩展思考内容
    ToolUse { tool_name, input, id },       // AI 决定调用工具
    ToolResult { id, output, is_error },    // 工具执行结果
    PermissionRequest { tool_name, input }, // 需要用户授权
    Result(QueryResult),                    // 最终结果
    Error(EngineError),                     // 错误
    ModelFallback { from, to },             // 模型降级
    StateUpdate { patch },                  // 状态更新
}
```

这种设计让客户端（终端 CLI、Telegram）可以实时展示 Agent 的每一步动作，而不是等到全部完成才返回。

## 2.4 与其他框架的对比

| 特性 | BaoClaw | LangChain | Claude Code |
|------|---------|-----------|-------------|
| 循环实现 | Rust async loop | Python chain | TypeScript loop |
| 流式输出 | SSE events via IPC | Callbacks | Direct stdout |
| 工具执行 | 并行 + 权限控制 | 串行 | 并行 |
| 错误恢复 | Fallback + auto-compact | 手动重试 | 手动重试 |

## 2.5 小结

ReAct 循环是 Agent 的核心。理解了这个循环，你就理解了 Agent 系统的骨架。接下来的章节将深入循环中的每个组件 —— 工具系统、上下文管理、记忆系统 —— 看它们如何协同工作。
