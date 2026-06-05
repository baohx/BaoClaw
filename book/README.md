# Agent Harness 实战：从 BaoClaw 看 AI Agent 系统架构

> 以一个真实的 Agent Harness 产品为原型，深入剖析 AI Agent 系统的设计模式与工程实践。
>
> 🐾 **BaoClaw 开源地址：[github.com/baohx/BaoClaw](https://github.com/baohx/BaoClaw)** (MIT License, v2.0.0)
>
> 📘 **在线阅读：[book2 演示版](book2/)** — HTML 幻灯片风格呈现，支持键盘导航和主题切换

## 关于本书

本书以 BaoClaw —— 一个基于 Rust + TypeScript 构建的 AI 编码助手（v1.0.0 正式版）—— 作为参考实现，系统讲解 Agent Harness（Agent 运行时框架）的架构设计、核心模式和生产实践。

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
| baoclaw-core | Rust | 全局守护进程、多项目 Session 管理、QueryEngine、工具执行、IPC 服务、Cron 调度、进化引擎、沙箱执行、跨会话检索 |
| ts-ipc | TypeScript | 终端 CLI 客户端（按 cwd 自动路由到对应项目 session） |
| baoclaw-telegram | TypeScript | Telegram 网关客户端（支持 /cd 切换项目） |
| baoclaw-whatsapp | TypeScript | WhatsApp 网关客户端 |
| baoclaw-feishu | TypeScript | 飞书网关客户端 |

**GitHub**: [github.com/baohx/BaoClaw](https://github.com/baohx/BaoClaw) (v1.0.0)

架构特点：**一个全局 daemon 管理所有项目**。每个项目目录对应独立的 session（独立对话历史、记忆、配置）。多个 CLI 终端和 Telegram 可同时连接，按工作目录自动路由到正确的 session，互不干扰。

核心特性：持久记忆、全局守护进程多项目会话、自我进化引擎（Skill 自动生成 + RLHF 数据导出）、Cron 定时任务、文档问答（PDF/DOCX/图片）、项目级隔离、200+ 模型支持、沙箱执行（Bubblewrap/Docker）、跨会话检索（SQLite FTS5）、自适应 Compact、工具健康监控、意图预测、上下文窗口分配器、Prompt 注入检测、Subagent 深度策略。

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

### Part 7：智能引擎 v2.0
- [第 23 章：跨会话检索与缓存优化](Part7-智能引擎v2/ch23-跨会话检索与缓存.md)
- [第 24 章：用户画像与 Skill 自改进闭环](Part7-智能引擎v2/ch24-用户画像与自改进闭环.md)
- [第 25 章：自适应引擎](Part7-智能引擎v2/ch25-自适应引擎.md)
- [第 26 章：安全与沙箱](Part7-智能引擎v2/ch26-安全与沙箱.md)

### 附录
- [附录 A：BaoClaw 完整架构图](附录/appendix-a-架构图.md)
- [附录 B：从零搭建 Agent Harness](附录/appendix-b-从零搭建.md)
- [附录 C：与其他框架的对比](附录/appendix-c-框架对比.md)
- [附录 D：配置文件完整参考](附录/appendix-d-配置文件参考.md)

## 写作理念

**模式优先，代码为证。**

每一章遵循这样的结构：
1. **问题** —— 什么场景需要这个能力？
2. **模式** —— 通用的设计模式是什么？
3. **实现** —— BaoClaw 是怎么做的？（附真实代码）
4. **思考** —— 还有哪些替代方案？

## v2.0 新增特性

BaoClaw v2.0 引入了智能引擎层，包含以下新功能：

### 🔍 跨会话检索 (#5)
- SQLite + FTS5 全文搜索，跨所有历史会话检索
- 按关键词搜索，获取排名结果和上下文片段

### ❄️ 冻结快照缓存 (#6)
- 系统提示词和工具列表在会话开始时冻结
- 最大化 Anthropic Prompt Cache 命中率
- 降低成本和延迟

### 👤 用户画像 (#7)
- `~/.baoclaw/USER.md` 持久化用户画像
- 自动加载到系统提示词，个性化响应
- 会话统计自动合并（总轮次、成本、常用工具）

### 🔄 Skill 自改进闭环 (#8)
- 5 阶段循环：收集 → 评估 → 改进 → 验证 → 淘汰
- 根据相关率、成功率、用户评分自动调整技能库

### 📐 自适应 Compact (#9)
- `AdaptiveCompactTracker` 从压缩历史学习最优 `keep_recent`
- 范围 6–30 条消息，自动调整

### 🏥 工具健康监控 (#10)
- 实时跟踪每个工具的成功/失败/超时率
- 三种状态：健康 → 降级（连续 3 次失败）→ 禁用（6 次失败）
- 降级工具在系统提示词中获得警告信息

### 🎯 意图预测 (#11)
- 从消息关键词预测用户意图（编码、调试、测试、重构、git、研究...）
- 转移矩阵学习意图之间的典型转换

### 🧮 上下文窗口分配器 (#12)
- 注意力分数 = 0.5×相关性 + 0.3×时效性 + 0.2×频率
- 强制块（系统提示词、工具）始终包含
- 可选块（记忆、技能、搜索结果）按分数贪心填充

### 🏖️ 沙箱执行 (#13)
- 三种后端：Bubblewrap（Linux 命名空间）→ Docker（容器）→ None（直接）
- 自动检测最佳可用后端
- 可配置：只读/读写挂载、网络隔离、内存/CPU 限制、超时

### 🛡️ Prompt 注入检测 (#14)
- 20 种模式，6 个类别：指令覆盖、角色劫持、数据外泄、编码技巧、隐藏载荷、越狱
- 四个严重级别：干净 → 可疑 → 危险 → 严重

### 🔐 Subagent 深度策略 (#15)
- 最大嵌套深度：3 层
- 渐进式工具限制：深度 0 = 所有工具，深度 1 = 安全工具，深度 2 = 只读，深度 3 = 最小
- 每层预算限制

### 📡 流式工具执行器 (#16)
- 实时分块输出：Started → Progress → Stdout → Stderr → Completed → Error → Heartbeat
- 可配置超时、缓冲区大小、最大输出

## License

本书内容采用 CC BY-NC-SA 4.0 协议。

BaoClaw 源代码采用 MIT 协议：[github.com/baohx/BaoClaw](https://github.com/baohx/BaoClaw)
