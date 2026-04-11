# 附录 C：与其他框架的对比

## Agent Harness 框架对比

| 特性 | BaoClaw | Claude Code | OpenClaw | Shannon | LangGraph |
|------|---------|-------------|----------|---------|-----------|
| 语言 | Rust + TS | TypeScript | TypeScript | Rust + Go + Python | Python |
| 架构 | Daemon + IPC | 单进程 | Daemon + IPC | 三层微服务 | 库 |
| 多客户端 | ✅ CLI + Telegram | ❌ CLI only | ✅ CLI + Telegram | ✅ API | ❌ |
| 共享会话 | ✅ SharedSession | ❌ | ✅ | ✅ | ❌ |
| MCP 支持 | ✅ | ✅ | ✅ | ✅ | ❌ |
| Skills | ✅ 系统提示词注入 | ❌ | ✅ | ❌ | ❌ |
| 长期记忆 | ✅ JSONL | ❌ | ✅ | ✅ 向量数据库 | ❌ |
| 模型 Fallback | ✅ | ❌ | ✅ | ✅ | ❌ |
| 自动 Compact | ✅ | ❌ | ✅ | ❌ | ❌ |
| Computer Use | ✅ via MCP | ✅ 内置 | ✅ via MCP | ❌ | ❌ |
| 权限控制 | ✅ PermissionGate | ✅ | ✅ | ✅ OPA | ❌ |
| 成本追踪 | ✅ | ✅ | ✅ | ✅ | ❌ |
| 多 Agent 编排 | ❌ | ❌ | ❌ | ✅ Temporal | ✅ DAG |
| 沙箱隔离 | ❌ | ❌ | ❌ | ✅ WASI | ❌ |

## 选型建议

- **个人开发者**：BaoClaw / Claude Code —— 轻量、快速上手
- **团队协作**：Shannon —— 企业级治理、多租户
- **快速原型**：LangGraph —— Python 生态、灵活
- **多客户端需求**：BaoClaw / OpenClaw —— Daemon 架构天然支持

## BaoClaw 的独特之处

1. **Rust Daemon** —— 高性能、低内存、长时间运行稳定
2. **真正的多客户端共享** —— 终端和 Telegram 实时共享同一个对话
3. **Skill 系统** —— 零代码扩展 Agent 行为
4. **自动记忆** —— AI 主动保存用户偏好和项目规则
5. **图片处理** —— MCP 截图 → base64 剥离 → Telegram 图片发送的完整链路
