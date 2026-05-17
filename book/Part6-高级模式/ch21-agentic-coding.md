# 第 21 章：Agentic Coding —— 代码生成与编辑

## 21.1 Agent 写代码

Agentic Coding 是 Agent 最核心的应用场景之一。不是简单的代码补全，而是 Agent 自主地：读取项目结构、理解代码、创建文件、修改代码、运行测试、修复错误。

## 21.2 BaoClaw 的代码工具链

| 工具 | 职责 | 典型用法 |
|------|------|----------|
| FileReadTool | 读取文件 | 理解现有代码 |
| FileWriteTool | 创建文件 | 生成新代码文件 |
| FileEditTool | 编辑文件 | 搜索替换修改代码 |
| GrepTool | 搜索代码 | 找到相关函数/类 |
| GlobTool | 文件发现 | 了解项目结构 |
| BashTool | 运行命令 | 编译、测试、安装依赖 |
| AgentTool | 子 Agent | 委托只读任务 |

### FileEditTool 的设计

文件编辑是最复杂的工具。BaoClaw 使用搜索替换模式：

```json
{
    "command": "str_replace",
    "path": "src/main.rs",
    "old_str": "fn old_function() {",
    "new_str": "fn new_function() {"
}
```

为什么不用行号？因为 AI 在多轮对话中，文件内容可能已经被之前的编辑改变了，行号不可靠。搜索替换基于内容匹配，更稳定。

### AgentTool —— 子 Agent

复杂任务可以委托给子 Agent：

```rust
pub struct AgentTool {
    api_client: Arc<AnthropicClient>,
    read_only_tools: Vec<Arc<dyn Tool>>,  // 只给子 Agent 只读工具
}
```

子 Agent 只有 FileRead、Grep、Glob、WebFetch 四个工具，不能写文件或执行命令。这是安全隔离 —— 子 Agent 只能"看"不能"做"。

## 21.3 项目上下文

AI 写代码需要理解项目。BaoClaw 通过多种方式提供项目上下文：

1. **BAOCLAW.md** —— 项目规则（"用 Rust"、"测试用 proptest"）
2. **Git 状态** —— 当前分支、已修改文件
3. **Skills** —— 项目特定的工作流指令
4. **ProjectNoteTool** —— AI 自动发现并记录项目规则

## 21.4 典型工作流

用户说"给这个函数加单元测试"：

```
1. AI 调用 GrepTool 搜索函数定义
2. AI 调用 FileReadTool 读取函数代码
3. AI 调用 FileReadTool 读取现有测试文件（了解测试风格）
4. AI 调用 FileEditTool 在测试文件中追加新测试
5. AI 调用 BashTool 运行 cargo test
6. 如果测试失败：AI 读取错误信息，修改代码，重新运行
7. AI 回复"测试已添加并通过"
```

这个过程完全自主 —— 用户只说了一句话，AI 完成了 6-7 步操作。

## 21.5 小结

Agentic Coding 是 ReAct 循环 + 代码工具链的自然结合。关键是提供足够的项目上下文（BAOCLAW.md、Git 状态）和安全的工具集（权限控制、子 Agent 隔离）。
