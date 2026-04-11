# Agent Harness 实战：从 BaoClaw 看 AI Agent 系统架构

> 以一个真实的 Agent Harness 产品为原型，深入剖析 AI Agent 系统的设计模式与工程实践。

## 关于本书

本书以 BaoClaw —— 一个基于 Rust + TypeScript 构建的 AI 编码助手 —— 作为参考实现，系统讲解 Agent Harness（Agent 运行时框架）的架构设计、核心模式和生产实践。

与市面上大多数 Agent 教程不同，本书不是 API 调用指南或 Prompt 技巧集锦，而是从工程角度回答：

- 一个 Agent 系统的核心循环是什么？
- 工具系统如何设计才能可扩展？
- 多客户端如何共享同一个 Agent 会话？
- 记忆系统如何跨会话持久化？
- MCP 协议如何让 Agent 获得无限能力？
- 生产环境中如何处理上下文窗口、错误恢复、并发控制？

每一个问题，都有 BaoClaw 的真实代码作为答案。

## 参考实现：BaoClaw

BaoClaw 是一个完整的 Agent Harness 系统，包含：

| 组件 | 技术栈 | 职责 |
|------|--------|------|
| claude-core | Rust | 守护进程、QueryEngine、工具执行、IPC 服务 |
| ts-ipc | TypeScript | 终端 CLI 客户端 |
| baoclaw-telegram | TypeScript | Telegram 网关客户端 |

## 目录

### Part 1：Agent 基础
- [第 1 章：Agent 的本质 —— 从 LLM 到 Agent](Part1-Agent基础/ch01-agent的本质.md)
- [第 2 章：ReAct 循环 —— Agent 的心跳](Part1-Agent基础/ch02-react循环.md)
- [第 3 章：Agent Harness 架构概览](Part1-Agent基础/ch03-harness架构概览.md)

### Part 2：工具与扩展
- [第 4 章：工具系统设计 —— Tool Trait 与执行器](Part2-工具与扩展/ch04-工具系统设计.md)
- [第 5 章：MCP 协议 —— 让 Agent 获得无限能力](Part2-工具与扩展/ch05-mcp协议.md)
- [第 6 章：Skills —— 可插拔的 Agent 行为](Part2-工具与扩展/ch06-skills.md)
- [第 7 章：Plugins —— 打包分发的能力套件](Part2-工具与扩展/ch07-plugins.md)

### Part 3：上下文与记忆
- [第 8 章：上下文管理 —— 系统提示词的构建](Part3-上下文与记忆/ch08-上下文管理.md)
- [第 9 章：短期记忆 —— 对话历史与 Compact](Part3-上下文与记忆/ch09-短期记忆.md)
- [第 10 章：长期记忆 —— 跨会话的知识持久化](Part3-上下文与记忆/ch10-长期记忆.md)
- [第 11 章：会话设计 —— 共享、恢复与多客户端](Part3-上下文与记忆/ch11-会话设计.md)

### Part 4：IPC 与多客户端
- [第 12 章：守护进程架构 —— Daemon 模式](Part4-IPC与多客户端/ch12-守护进程架构.md)
- [第 13 章：IPC 协议 —— JSON-RPC over UDS](Part4-IPC与多客户端/ch13-ipc协议.md)
- [第 14 章：多客户端接入 —— 终端、Telegram、更多](Part4-IPC与多客户端/ch14-多客户端接入.md)
- [第 15 章：共享会话 —— SharedQueryEngine 的设计](Part4-IPC与多客户端/ch15-共享会话.md)

### Part 5：生产实践
- [第 16 章：错误处理与恢复 —— 从 Fallback 到自动 Compact](Part5-生产实践/ch16-错误处理与恢复.md)
- [第 17 章：流式输出 —— SSE 事件与广播](Part5-生产实践/ch17-流式输出.md)
- [第 18 章：权限控制 —— PermissionGate 模式](Part5-生产实践/ch18-权限控制.md)
- [第 19 章：成本追踪 —— Token 计量与预算](Part5-生产实践/ch19-成本追踪.md)

### Part 6：高级模式
- [第 20 章：Computer Use —— 桌面控制 Agent](Part6-高级模式/ch20-computer-use.md)
- [第 21 章：Agentic Coding —— 代码生成与编辑](Part6-高级模式/ch21-agentic-coding.md)
- [第 22 章：多模型支持 —— Fallback 与模型切换](Part6-高级模式/ch22-多模型支持.md)

### 附录
- [附录 A：BaoClaw 完整架构图](附录/appendix-a-架构图.md)
- [附录 B：从零搭建 Agent Harness](附录/appendix-b-从零搭建.md)
- [附录 C：与其他框架的对比](附录/appendix-c-框架对比.md)

## 写作理念

**模式优先，代码为证。**

每一章遵循这样的结构：
1. **问题** —— 什么场景需要这个能力？
2. **模式** —— 通用的设计模式是什么？
3. **实现** —— BaoClaw 是怎么做的？（附真实代码）
4. **思考** —— 还有哪些替代方案？

## License

本书内容采用 CC BY-NC-SA 4.0 协议。
