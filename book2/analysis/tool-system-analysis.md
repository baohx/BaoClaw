# BaoClaw 工具系统源码分析

**任务:** 14.3 阅读工具系统源码  
**需求:** 9.1, 9.2  
**日期:** 2025-01-20  
**源码路径:**
- `baoclaw-core/src/tools/` - 工具实现
- `baoclaw-core/src/mcp/` - MCP 客户端实现

---

## 1. 工具系统架构概览

BaoClaw 的工具系统采用 **Trait-based Plugin Architecture**，核心是 `Tool` trait，所有工具（内置工具和 MCP 工具）都实现该 trait。

```
┌─────────────────────────────────────────────────────────────┐
│                      Tool System                             │
│  ┌─────────────────┐                                        │
│  │   Tool Trait    │  ← 核心抽象 (trait_def.rs)             │
│  │  - name()       │                                        │
│  │  - call()       │                                        │
│  │  - validate()   │                                        │
│  │  - prompt()     │                                        │
│  └────────┬────────┘                                        │
│           │                                                  │
│  ┌────────┴────────┬─────────────────┐                     │
│  │                 │                 │                      │
│  ▼                 ▼                 ▼                      │
│ Built-in Tools   MCP Tools        Agent Tools              │
│ (Rust impl)      (McpToolWrapper)  (sub-agents)            │
│  │                 │                 │                      │
│  ├── BashTool     ├── [MCP Server  ├── AgentTool           │
│  ├── FileRead     │   tools]       │   (creates sub-       │
│  ├── FileWrite    └── McpToolWrapper  QueryEngine)         │
│  ├── FileEdit         (adapts MCP                           │
│  ├── Glob             to Tool trait)                        │
│  ├── Grep                                                   │
│  ├── WebFetch                                               │
│  ├── WebSearch                                              │
│  ├── SkillTool                                              │
│  ├── MemoryTool                                             │
│  └── ...                                                    │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Tool Trait 定义

**源文件:** `baoclaw-core/src/tools/trait_def.rs`

### 2.1 核心类型

```rust
// 文件: trait_def.rs (行 1-30)
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Tool execution result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub data: Value,
    pub is_error: bool,
}

/// Input validation result
#[derive(Clone, Debug)]
pub enum ValidationResult {
    Ok,
    Invalid { message: String, code: Option<String> },
}

/// Permission check result from a tool's perspective
#[derive(Clone, Debug)]
pub enum ToolPermissionCheckResult {
    Allow { updated_input: Value },
    Ask { message: String, updated_input: Value },
    Deny { message: String },
}
```

### 2.2 Tool Trait 定义

```rust
// 文件: trait_def.rs (行 55-140)
/// The core Tool trait that all tools must implement
#[async_trait]
pub trait Tool: Send + Sync {
    /// The unique name of this tool
    fn name(&self) -> &str;

    /// Alternative names for this tool
    fn aliases(&self) -> Vec<&str> { vec![] }

    /// JSON Schema for the tool's input
    fn input_schema(&self) -> JsonSchema;

    /// Whether this tool only reads data (doesn't modify filesystem)
    fn is_read_only(&self, _input: &Value) -> bool { false }

    /// Whether this tool is destructive (e.g., deletes files)
    fn is_destructive(&self, _input: &Value) -> bool { false }

    /// Whether this tool can be safely executed concurrently
    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }

    /// Maximum result size in characters before persisting to disk
    fn max_result_size_chars(&self) -> usize { 100_000 }

    /// Execute the tool with the given input
    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError>;

    /// Validate the input before execution
    async fn validate_input(&self, _input: &Value, _context: &ToolContext) -> ValidationResult {
        ValidationResult::Ok
    }

    /// Tool-specific permission check
    async fn check_permissions(&self, _input: &Value, _context: &ToolContext) 
        -> ToolPermissionCheckResult { /* default implementation */ }

    /// Get the system prompt contribution for this tool
    fn prompt(&self) -> String;

    /// Whether this tool should be deferred (lazy-loaded) in the prompt
    fn is_deferred(&self) -> bool { false }
}
```

### 2.3 ToolContext - 工具执行上下文

```rust
/// Context available to tools during execution
#[derive(Clone)]
pub struct ToolContext {
    pub cwd: PathBuf,
    pub model: String,
    pub abort_signal: Arc<tokio::sync::watch::Receiver<bool>>,
    /// Shared file cache for reducing redundant file reads
    pub file_cache: Option<Arc<Mutex<FileCache>>>,
    /// Tool result store for persisting large outputs to disk
    pub tool_result_store: Option<Arc<ToolResultStore>>,
}
```

### 2.4 ToolError - 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Tool timed out after {0}ms")]
    Timeout(u64),
    #[error("Tool was aborted")]
    Aborted,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
```

---

## 3. 工具执行器 (Tool Executor)

**源文件:** `baoclaw-core/src/tools/executor.rs`

### 3.1 执行流程

```
ToolUseRequest
      │
      ▼
┌─────────────┐
│  validate   │  ← 验证输入参数
└─────┬───────┘
      │ Validation::Ok
      ▼
┌─────────────┐
│ permissions │  ← 检查权限 (PermissionManager)
└─────┬───────┘
      │ Allow
      ▼
┌─────────────┐
│    call     │  ← 执行工具 (tool.call())
└─────┬───────┘
      │ Result
      ▼
┌─────────────┐
│   persist   │  ← 大结果持久化 (可选)
└─────┬───────┘
      │
      ▼
ToolExecutionResult
```

### 3.2 核心执行函数

```rust
// 文件: executor.rs (行 62-100)
/// Execute a single tool: validate → permissions → call
pub async fn execute_tool(
    tool: &dyn Tool,
    request: &ToolUseRequest,
    context: &ToolContext,
    progress: &dyn ProgressSender,
) -> ToolExecutionResult {
    // Step 1: Validate input
    let validation = tool.validate_input(&request.input, context).await;
    if let ValidationResult::Invalid { message, .. } = validation {
        return ToolExecutionResult {
            tool_use_id: request.id.clone(),
            tool_name: tool.name().to_string(),
            output: Value::String(format!("Validation error: {}", message)),
            is_error: true,
        };
    }

    // Step 2: Check permissions
    let permission = tool.check_permissions(&request.input, context).await;
    if let ToolPermissionCheckResult::Deny { message } = permission {
        return ToolExecutionResult { /* permission denied */ };
    }

    // Step 3: Call the tool with abort awareness
    let call_result = tokio::select! {
        r = tool.call(request.input.clone(), context, progress) => r,
        _ = abort_signal => Err(ToolError::Aborted),
    };
    // ... handle result
}
```

### 3.3 并发执行策略

```rust
/// Execute multiple tools with concurrency optimization
pub async fn execute_tools(
    tools: &[Arc<dyn Tool>],
    requests: &[ToolUseRequest],
    context: &ToolContext,
    progress: &dyn ProgressSender,
) -> Vec<ToolExecutionResult> {
    // Classify tools into concurrent-safe vs sequential
    let mut concurrent: Vec<_> = Vec::new();
    let mut sequential: Vec<_> = Vec::new();

    for (idx, req) in requests.iter().enumerate() {
        match find_tool(tools, &req.name) {
            Some(tool) => {
                if tool.is_concurrency_safe(&req.input) {
                    concurrent.push((idx, req, tool));
                } else {
                    sequential.push((idx, req, tool));
                }
            }
            None => { /* handle not found */ }
        }
    }

    // Execute concurrent-safe tools in parallel using futures::join_all
    let concurrent_results = futures::future::join_all(
        concurrent.iter().map(|(_, req, tool)| execute_tool(...))
    ).await;

    // Execute sequential tools one by one
    for (idx, req, tool) in &sequential {
        results[*idx] = Some(execute_tool(...).await);
    }
}
```

---

## 4. 内置工具实现

### 4.1 工具注册表

**源文件:** `baoclaw-core/src/tools/builtins/mod.rs`

| 工具名 | 别名 | 只读 | 并发安全 | 功能描述 |
|--------|------|------|----------|----------|
| BashTool | Bash | ❌ | ❌ | 执行 Shell 命令，支持沙箱 |
| FileReadTool | Read | ✅ | ✅ | 读取文件内容，支持行范围 |
| FileWriteTool | Write | ❌ | ❌ | 写入文件，自动创建目录 |
| FileEditTool | Edit | ❌ | ❌ | 查找替换文件内容 |
| GlobTool | Glob, FindFiles | ✅ | ✅ | 文件名通配符搜索 |
| GrepTool | Grep, Search | ✅ | ✅ | 内容正则搜索，支持 .gitignore |
| WebFetchTool | Fetch | ✅ | ✅ | HTTP 请求，HTML 转文本 |
| WebSearchTool | WebSearch | ✅ | ✅ | 网页搜索 |
| SkillTool | Skill, LoadSkill | ✅ | ✅ | 延迟加载技能文件 |
| MemoryTool | Memory, SaveMemory | ❌ | ✅ | 长期记忆存储 |
| AgentTool | Agent | ❌ | ✅ | 创建子代理执行任务 |

### 4.2 BashTool 设计要点

**源文件:** `baoclaw-core/src/tools/builtins/bash_tool.rs`

```rust
pub struct BashTool {
    sandbox: Option<Arc<SandboxConfig>>,  // 可选沙箱配置
}

impl Tool for BashTool {
    fn name(&self) -> &str { "Bash" }
    fn is_concurrency_safe(&self) -> bool { false }

    async fn validate_input(&self, input: &Value, ...) -> ValidationResult {
        // 安全检查: 阻止危险命令
        if let Err(reason) = check_dangerous_command(cmd) {
            return ValidationResult::Invalid { 
                message: format!("Command blocked: {}", reason),
                code: Some("DANGEROUS_COMMAND".to_string()),
            };
        }
    }

    async fn call(&self, input: Value, ...) -> Result<ToolResult, ToolError> {
        // 构建命令 (沙箱或直接执行)
        let (program, args) = self.build_command(command, &context.cwd);
        
        // 竞态: 等待子进程 vs 超时 vs 取消信号
        let result = tokio::select! {
            r = child.wait_with_output() => r,
            _ = abort_signal => { /* 杀死子进程 */ }
        };
    }
}
```

**关键特性:**
1. **沙箱支持:** 可配置 Docker/Bubblewrap 隔离
2. **安全检查:** `check_dangerous_command()` 阻止危险命令
3. **超时控制:** 默认 120 秒，可配置
4. **Abort 支持:** 响应取消信号，杀死子进程

### 4.3 FileReadTool 设计要点

**源文件:** `baoclaw-core/src/tools/builtins/file_read_tool.rs`

```rust
pub struct FileReadTool {
    additional_dirs: Vec<PathBuf>,  // 允许的额外目录
}

impl Tool for FileReadTool {
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }
    fn max_result_size_chars(&self) -> usize { 30_000 }  // ~7.5k tokens

    async fn call(&self, input: Value, context: &ToolContext, ...) {
        // 路径安全验证
        let resolved = resolve_and_validate_path(file_path, &context.cwd, &self.additional_dirs)?;

        // 文件缓存检查 (仅全文件读取)
        if offset == 0 && limit.is_none() {
            if let Some(ref cache_arc) = context.file_cache {
                match cache.check(&resolved) {
                    CacheStatus::Hit => return cached_content,
                    _ => { /* 读取并更新缓存 */ }
                }
            }
        }
    }
}
```

**关键特性:**
1. **路径安全:** `resolve_and_validate_path()` 防止路径遍历
2. **文件缓存:** 集成 `FileCache` 减少重复读取
3. **行范围:** 支持 `offset`/`limit` 参数

### 4.4 SkillTool - 延迟加载技能

**源文件:** `baoclaw-core/src/tools/builtins/skill_tool.rs`

```rust
pub struct SkillTool {
    cwd: PathBuf,
}

impl Tool for SkillTool {
    fn name(&self) -> &str { "Skill" }
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, ...) -> Result<ToolResult, ToolError> {
        let skill_name = input.get("skill").and_then(|v| v.as_str())?;

        // 特殊功能: 列出所有可用技能
        if skill_name == "__list__" {
            let skills = discover_skills(&self.cwd).await;
            return Ok(ToolResult { /* 技能列表 */ });
        }

        // 搜索并加载技能文件
        match self.find_skill(skill_name).await {
            Some((content, source)) => Ok(ToolResult {
                data: json!({ "success": true, "content": content, "source": source }),
                is_error: false,
            }),
            None => Ok(ToolResult { /* 未找到 */ })
        }
    }

    /// 搜索顺序: 用户级 (~/.baoclaw/skills/) → 项目级 (<cwd>/.baoclaw/skills/)
    async fn find_skill(&self, name: &str) -> Option<(String, String)> {
        // 先尝试 <name>/SKILL.md 格式，再尝试 <name>.md 格式
    }
}
```

**设计要点:**
1. **延迟加载:** 技能内容按需加载，不预加载到系统提示
2. **多层级搜索:** 用户级 → 项目级
3. **列表功能:** `__list__` 特殊值列出所有技能

### 4.5 AgentTool - 子代理执行

**源文件:** `baoclaw-core/src/tools/builtins/agent_tool.rs`

```rust
pub struct AgentTool {
    api_client: Arc<UnifiedClient>,
    available_tools: Vec<Arc<dyn Tool>>,  // 子代理可用的工具集
    default_max_turns: u32,                // 默认最大轮次
}

impl Tool for AgentTool {
    fn name(&self) -> &str { "AgentTool" }
    fn is_concurrency_safe(&self) -> bool { true }  // 子代理可并发执行

    async fn call(&self, input: Value, context: &ToolContext, progress: &dyn ProgressSender) {
        let prompt = input["prompt"].as_str()?;
        let max_turns = input["max_turns"].as_u64().unwrap_or(10) as u32;

        // 创建独立的子代理 QueryEngine
        let sub_engine_config = QueryEngineConfig {
            cwd: context.cwd.clone(),
            tools: self.available_tools.clone(),  // 完整工具集
            api_client: Arc::clone(&self.api_client),
            max_turns: Some(max_turns),
            custom_system_prompt: Some("You are a sub-agent...".to_string()),
            // ... 其他配置
        };

        let mut sub_engine = QueryEngine::new(sub_engine_config);
        let mut rx = sub_engine.submit_message(prompt).await;

        // 收集子代理输出并转发进度
        while let Some(event) = rx.recv().await {
            match event {
                EngineEvent::ToolUse { tool_name, .. } => {
                    progress.send_progress(...).await;  // 转发给父代理
                }
                EngineEvent::Result(result) => break,
                // ...
            }
        }

        Ok(ToolResult {
            data: json!({ "result": final_text, "cost_usd": total_cost }),
            is_error: false,
        })
    }
}
```

**设计要点:**
1. **独立上下文:** 子代理有独立的对话历史
2. **完整工具集:** 子代理可使用所有工具
3. **进度转发:** 子代理的工具调用转发给父代理
4. **成本追踪:** 返回子代理的 API 成本

---

## 5. MCP 客户端实现

### 5.1 架构概览

```
┌─────────────────────────────────────────────────────────────┐
│                      MCP Client                              │
│  ┌───────────────┐                                          │
│  │  McpClient    │  ← MCP 客户端 (client.rs)                │
│  │  - connect()  │                                          │
│  │  - list_tools │                                          │
│  │  - call_tool  │                                          │
│  └───────┬───────┘                                          │
│          │                                                   │
│  ┌───────┴───────┐                                          │
│  │ StdioTransport │  ← 进程间通信 (transport.rs)            │
│  │  - spawn()    │                                          │
│  │  - request()  │                                          │
│  └───────────────┘                                          │
│          │                                                   │
│  ┌───────┴───────┐                                          │
│  │ McpToolWrapper │  ← 适配 Tool trait (tool_wrapper.rs)    │
│  └───────────────┘                                          │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 McpClient 核心实现

**源文件:** `baoclaw-core/src/mcp/client.rs`

```rust
pub struct McpServerConfig {
    pub name: String,
    pub command: String,          // MCP 服务器启动命令
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub transport: McpTransportType,  // Stdio 或 Sse
}

pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,      // JSON Schema
}

pub struct McpClient {
    config: McpServerConfig,
    status: Arc<RwLock<McpConnectionStatus>>,
    tools: Arc<RwLock<Vec<McpToolDef>>>,
    transport: Option<Arc<RwLock<StdioTransport>>>,
}

impl McpClient {
    /// 通过 stdio 连接: 启动子进程 → 握手 → 刷新工具列表
    pub async fn connect_stdio(&mut self) -> Result<(), McpError> {
        let transport = StdioTransport::spawn(
            &self.config.command,
            &self.config.args,
            &self.config.env,
        ).await?;

        self.transport = Some(Arc::new(RwLock::new(transport)));
        *self.status.write().await = McpConnectionStatus::Connected;

        self.refresh_tools().await?;
        Ok(())
    }

    /// 从 MCP 服务器获取工具列表
    pub async fn refresh_tools(&self) -> Result<(), McpError> {
        let transport = self.transport.as_ref().ok_or(McpError::NotConnected)?;
        let result = transport.write().await.request("tools/list", None).await?;
        let tool_defs: Vec<McpToolDef> = serde_json::from_value(result["tools"].clone())?;
        *self.tools.write().await = tool_defs;
        Ok(())
    }

    /// 调用 MCP 工具
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, McpError> {
        let transport = self.transport.as_ref().ok_or(McpError::NotConnected)?;
        transport.write().await.request("tools/call", Some(json!({
            "name": name,
            "arguments": args,
        }))).await
    }
}
```

### 5.3 StdioTransport - JSON-RPC 通信

**源文件:** `baoclaw-core/src/mcp/transport.rs`

```rust
pub struct StdioTransport {
    child: Child,                    // 子进程
    writer: BufWriter<ChildStdin>,   // 写入 stdin
    reader: BufReader<ChildStdout>,  // 读取 stdout
    next_id: u64,                    // JSON-RPC 请求 ID
}

impl StdioTransport {
    /// 启动子进程并执行 MCP 初始化握手
    pub async fn spawn(command: &str, args: &[String], env: &HashMap<String, String>) 
        -> Result<Self, McpError> {
        let mut child = tokio::process::Command::new(command)
            .args(args).envs(env)
            .stdin(Stdio::piped()).stdout(Stdio::piped())
            .spawn()?;

        let mut transport = Self { child, writer, reader, next_id: 1 };

        // MCP initialize 握手
        transport.request("initialize", Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "baoclaw", "version": "0.3.0" }
        }))).await?;

        transport.notify("notifications/initialized", None).await?;
        Ok(transport)
    }

    /// 发送 JSON-RPC 请求并等待响应
    pub async fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
        let request = McpJsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id,
            method: method.to_string(),
            params,
        };
        self.next_id += 1;

        // 写入请求
        self.writer.write_all(serde_json::to_string(&request)?.as_bytes()).await?;
        self.writer.flush().await?;

        // 读取响应 (跳过通知消息)
        loop {
            let response: McpJsonRpcResponse = serde_json::from_str(&self.reader.read_line().await?)?;
            if response.id.is_none() { continue; }  // 跳过通知
            return Ok(response.result.unwrap_or(Value::Null));
        }
    }
}
```

### 5.4 McpToolWrapper - 适配 Tool Trait

**源文件:** `baoclaw-core/src/mcp/tool_wrapper.rs`

```rust
/// 将 MCP 服务器工具包装为 BaoClaw Tool trait 实现
pub struct McpToolWrapper {
    client: Arc<McpClient>,
    tool_def: McpToolDef,
    server_name: String,
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str { &self.tool_def.name }

    fn input_schema(&self) -> JsonSchema {
        // 将 MCP JSON Schema 转换为 BaoClaw JsonSchema
        JsonSchema {
            schema_type: self.tool_def.input_schema["type"].as_str().unwrap_or("object").to_string(),
            properties: self.tool_def.input_schema.get("properties").cloned(),
            required: self.tool_def.input_schema.get("required").and_then(|v| 
                v.as_array().map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            ),
            description: Some(self.tool_def.description.clone()),
        }
    }

    async fn call(&self, input: Value, _context: &ToolContext, _progress: &dyn ProgressSender) 
        -> Result<ToolResult, ToolError> {
        let result = self.client.call_tool(&self.tool_def.name, input).await
            .map_err(|e| ToolError::ExecutionFailed(format!("MCP tool error: {}", e)))?;

        let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);

        Ok(ToolResult { data: result, is_error })
    }

    fn prompt(&self) -> String {
        format!("[MCP:{}] {}", self.server_name, self.tool_def.description)
    }

    /// MCP 工具可能返回大型 base64 图像，提高限制到 10MB
    fn max_result_size_chars(&self) -> usize { 10_000_000 }
}
```

---

## 6. 路径安全机制

**源文件:** `baoclaw-core/src/tools/builtins/path_utils.rs`

```rust
/// 解析并验证文件路径，防止路径遍历攻击
pub fn resolve_and_validate_path(
    path: &str,
    cwd: &Path,
    additional_dirs: &[PathBuf],
) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("Path cannot be empty".to_string());
    }

    let raw = Path::new(path);

    // 绝对路径: 直接允许
    if raw.is_absolute() {
        return Ok(normalize_path(raw));
    }

    // 相对路径: 解析后检查是否在允许的目录内
    let absolute = cwd.join(raw);
    let normalized = normalize_path(&absolute);

    if !is_within_boundaries(&normalized, cwd, additional_dirs) {
        return Err(format!("Path '{}' is outside allowed directories", path));
    }

    Ok(normalized)
}

/// 检查路径是否在允许的目录内
fn is_within_boundaries(path: &Path, cwd: &Path, additional_dirs: &[PathBuf]) -> bool {
    path.starts_with(&normalize_path(cwd)) 
        || additional_dirs.iter().any(|dir| path.starts_with(&normalize_path(dir)))
}
```

---

## 7. 设计模式总结

### 7.1 Trait-based Plugin Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  Tool trait (核心抽象)                                        │
│  ├── 内置工具: 直接实现 Tool trait                            │
│  │   ├── BashTool, FileReadTool, FileWriteTool, ...         │
│  │   └── 共享代码: path_utils, backup, ...                   │
│  │                                                           │
│  └── MCP 工具: 通过 McpToolWrapper 适配                       │
│      └── 将 MCP 工具定义转换为 Tool trait 实现                │
└──────────────────────────────────────────────────────────────┘
```

### 7.2 执行管道模式

每个工具调用遵循固定管道:
1. **验证阶段:** `validate_input()` 检查输入参数
2. **权限阶段:** `check_permissions()` 检查执行权限
3. **执行阶段:** `call()` 执行实际操作
4. **后处理:** 大结果持久化、截断

### 7.3 并发优化策略

```rust
// 工具分类
if tool.is_concurrency_safe(&input) {
    concurrent_tools.push(tool);  // 并行执行
} else {
    sequential_tools.push(tool);  // 串行执行
}

// 使用 futures::join_all 并行执行
let results = futures::future::join_all(concurrent_futures).await;
```

### 7.4 取消信号传播

```rust
// 在所有工具执行中监听取消信号
tokio::select! {
    result = tool.call(...) => result,
    _ = abort_signal => {
        // 清理资源 (杀死子进程、关闭连接等)
        Err(ToolError::Aborted)
    }
}
```

### 7.5 结果大小控制

```rust
// 工具可自定义最大结果大小
fn max_result_size_chars(&self) -> usize {
    match self {
        FileReadTool => 30_000,      // ~7.5k tokens
        McpToolWrapper => 10_000_000, // 10MB (支持 base64 图像)
        _ => 100_000,                 // 默认 100K
    }
}

// 超出时持久化或截断
fn maybe_persist_or_truncate(data: Value, max_size: usize, context: &ToolContext, tool_use_id: &str) -> Value {
    if serialized.len() <= max_size { return data; }
    
    if let Some(ref store) = context.tool_result_store {
        return store.persist_and_format(&content, tool_use_id);
    }
    
    truncate(data, max_size)
}
```

---

## 8. 关键文件索引

| 文件路径 | 功能描述 |
|----------|----------|
| `src/tools/mod.rs` | 工具模块入口 |
| `src/tools/trait_def.rs` | Tool trait 定义、ToolContext、ToolError |
| `src/tools/executor.rs` | 工具执行器、并发执行逻辑 |
| `src/tools/builtins/mod.rs` | 内置工具注册 |
| `src/tools/builtins/bash_tool.rs` | Shell 命令执行 |
| `src/tools/builtins/file_read_tool.rs` | 文件读取 |
| `src/tools/builtins/file_write_tool.rs` | 文件写入 |
| `src/tools/builtins/file_edit_tool.rs` | 文件编辑 |
| `src/tools/builtins/glob_tool.rs` | 文件名搜索 |
| `src/tools/builtins/grep_tool.rs` | 内容搜索 |
| `src/tools/builtins/skill_tool.rs` | 技能加载 |
| `src/tools/builtins/agent_tool.rs` | 子代理执行 |
| `src/tools/builtins/path_utils.rs` | 路径安全验证 |
| `src/mcp/mod.rs` | MCP 模块入口 |
| `src/mcp/client.rs` | MCP 客户端 |
| `src/mcp/transport.rs` | JSON-RPC 通信 |
| `src/mcp/tool_wrapper.rs` | MCP 工具适配器 |

---

## 9. 书籍撰写建议

### 9.1 核心实现章节 (02-core-implementation)

建议包含以下内容:

1. **Tool Trait 设计**
   - 为什么选择 trait 而非 enum
   - 方法设计: 必需方法 vs 默认方法
   - 异步执行与取消支持

2. **执行管道**
   - 验证 → 权限 → 执行 的设计原因
   - 错误处理策略
   - 大结果处理

3. **并发执行**
   - `is_concurrency_safe()` 的设计权衡
   - 读写工具的分类
   - `futures::join_all` 的使用

4. **MCP 集成**
   - 适配器模式的应用
   - JSON-RPC 通信实现
   - 外部工具的统一抽象

### 9.2 代码示例选取

优先选取以下代码片段:

1. `trait_def.rs`: Tool trait 完整定义 (行 55-140)
2. `executor.rs`: `execute_tool()` 函数 (行 62-100)
3. `executor.rs`: `execute_tools()` 并发执行 (行 233-280)
4. `bash_tool.rs`: 取消信号处理 (行 150-180)
5. `tool_wrapper.rs`: McpToolWrapper 实现 (行 1-80)
