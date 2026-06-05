# BaoClaw 核心引擎源码分析

> 本文档基于 `baoclaw-core/src/engine/` 目录下的源码，提取关键实现用于书籍撰写。
> 
> 分析日期：2025年
> 源码路径：`/home/baohx@spdbfl/BaoClaw/baoclaw-core/src/engine/`

## 目录

1. [QueryEngine - 核心查询引擎](#1-queryengine---核心查询引擎)
2. [MemoryStore - 长期记忆系统](#2-memorystore---长期记忆系统)
3. [SessionMemory - 会话记忆系统](#3-sessionmemory---会话记忆系统)
4. [TokenCounter - Token 计数器](#4-tokencounter---token-计数器)
5. [CostTracker - 成本追踪器](#5-costtracker---成本追踪器)
6. [TranscriptWriter - 会话转录](#6-transcriptwriter---会话转录)
7. [SharedSession - 多客户端会话](#7-sharedsession---多客户端会话)
8. [EvolutionEngine - 自我进化引擎](#8-evolutionengine---自我进化引擎)
9. [StreamingExecutor - 流式执行器](#9-streamingexecutor---流式执行器)
10. [SandboxConfig - 沙箱执行](#10-sandboxconfig---沙箱执行)
11. [Tool Trait - 工具系统](#11-tool-trait---工具系统)
12. [IPC 协议 - 进程间通信](#12-ipc-协议---进程间通信)

---

## 1. QueryEngine - 核心查询引擎

**源文件**: `query_engine.rs`

QueryEngine 是 BaoClaw 的心脏，负责编排 LLM 调用、工具执行和消息管理。

### 1.1 核心配置结构体

```rust
// 源文件: query_engine.rs, 行: 32-64
pub struct QueryEngineConfig {
    pub cwd: PathBuf,
    pub tools: Vec<Arc<dyn Tool>>,
    pub api_client: Arc<UnifiedClient>,
    pub model: String,
    pub thinking_config: ThinkingConfig,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub verbose: bool,
    pub custom_system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub session_id: Option<String>,
    pub fallback_models: Vec<String>,
    pub max_retries_per_model: u32,
    pub context_window: u64,
    pub auto_compact_threshold_ratio: f64,
    pub parent_turn_id: Option<u32>,
    pub agent_label: Option<String>,
    pub session_memory: Option<Arc<SessionMemory>>,
    pub file_cache: Option<Arc<Mutex<FileCache>>>,
    pub tool_result_store: Option<Arc<ToolResultStore>>,
}
```

### 1.2 思考模式配置

```rust
// 源文件: query_engine.rs, 行: 66-75
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum ThinkingConfig {
    #[serde(rename = "disabled")]
    Disabled,
    #[serde(rename = "adaptive")]
    Adaptive,
    #[serde(rename = "enabled")]
    Enabled { budget_tokens: u32 },
}
```

### 1.3 引擎事件枚举

```rust
// 源文件: query_engine.rs, 行: 78-125
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EngineEvent {
    AssistantChunk { content: String, tool_use_id: Option<String> },
    ThinkingChunk { content: String },
    ToolUse { tool_name: String, input: Value, tool_use_id: String },
    ToolResult { tool_use_id: String, output: Value, is_error: bool },
    PermissionRequest { tool_name: String, input: Value, tool_use_id: String },
    Progress { tool_use_id: String, data: Value },
    StateUpdate { patch: Value },
    ModelFallback { from_model: String, to_model: String },
    TurnStart { turn_id: u32, parent_turn_id: Option<u32>, agent_label: Option<String> },
    TurnEnd { turn_id: u32, duration_ms: u64, tool_count: u32, input_tokens: u64, output_tokens: u64 },
    Result(QueryResult),
    Error(EngineError),
}
```

### 1.4 QueryEngine 核心结构体

```rust
// 源文件: query_engine.rs, 行: 188-207
pub struct QueryEngine {
    config: QueryEngineConfig,
    messages: Vec<Message>,
    pending_messages: Option<Arc<Mutex<Vec<Message>>>>,
    abort_tx: watch::Sender<bool>,
    abort_rx: watch::Receiver<bool>,
    total_usage: Usage,
    token_counter: Arc<Mutex<TokenCounter>>,
    compact_fail_count: usize,
    cached_project_instructions: Option<String>,
    cached_rules_raw: Vec<CachedRule>,
    cached_git_info: Option<GitInfo>,
}
```

### 1.5 关键方法签名

```rust
impl QueryEngine {
    pub fn new(config: QueryEngineConfig) -> Self;
    pub fn abort(&self);
    pub fn get_messages(&self) -> &[Message];
    pub fn set_messages(&mut self, messages: Vec<Message>);
    pub async fn compact(&mut self) -> Result<CompactResult, EngineError>;
    pub async fn submit_message(&mut self, prompt: String) -> mpsc::Receiver<EngineEvent>;
}
```

### 1.6 自适应压缩追踪器

```rust
// 源文件: query_engine.rs, 行: 143-184
pub struct AdaptiveCompactTracker {
    pub history: Vec<CompactFeedback>,
    pub keep_recent: usize,
    pub avg_compression_ratio: f64,
    pub avg_loss_score: f64,
    pub compact_count: u32,
}
```

---

## 2. MemoryStore - 长期记忆系统

**源文件**: `memory.rs`

MemoryStore 实现持久化的长期记忆，支持全局和项目级别的记忆存储。

### 2.1 记忆条目结构体

```rust
// 源文件: memory.rs, 行: 11-34
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MemoryCategory {
    Fact,       // 事实
    Preference, // 偏好
    Decision,   // 决策
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub category: MemoryCategory,
    pub created_at: String,
    pub source: String,
}
```

### 2.2 MemoryStore 结构体

```rust
// 源文件: memory.rs, 行: 37-41
pub struct MemoryStore {
    entries: Mutex<Vec<MemoryEntry>>,
    file_path: Mutex<PathBuf>,
}
```

### 2.3 核心方法

```rust
impl MemoryStore {
    // 加载全局记忆 (~/.baoclaw/memory.jsonl)
    pub fn load() -> Self;
    
    // 加载项目级别记忆 (<cwd>/.baoclaw/memory.jsonl)
    pub fn load_for_project(cwd: &Path) -> Self;
    
    // 切换项目
    pub async fn switch_project(&self, cwd: &Path);
    
    // 添加记忆
    pub async fn add(&self, content: String, category: MemoryCategory, source: String) -> MemoryEntry;
    
    // 构建系统提示片段
    pub async fn build_prompt_fragment(&self) -> Option<String>;
}
```

---

## 3. SessionMemory - 会话记忆系统

**源文件**: `session_memory.rs`

SessionMemory 维护会话级别的滚动摘要，在会话期间通过后台 API 调用更新。

### 3.1 核心常量

```rust
// 源文件: session_memory.rs, 行: 14-21
const FIRST_UPDATE_THRESHOLD: usize = 6;  // 首次生成摘要前的最小轮次
const UPDATE_INTERVAL: usize = 10;         // 摘要更新间隔
const MAX_SUMMARY_CHARS: usize = 8000;     // 最大摘要长度
```

### 3.2 SessionMemory 结构体

```rust
pub struct SessionMemory {
    file_path: PathBuf,
    content: Mutex<String>,
    last_update_count: Mutex<usize>,
}
```

### 3.3 核心方法

```rust
impl SessionMemory {
    pub fn path_for(session_id: &str) -> PathBuf;
    pub fn load(session_id: &str) -> Self;
    pub fn get(&self) -> String;
    pub fn is_available(&self) -> bool;
    pub fn should_update(&self, message_count: usize) -> bool;
    pub fn update(&self, summary: String);
}
```

---

## 4. TokenCounter - Token 计数器

**源文件**: `token_counter.rs`

TokenCounter 实现精确的 token 计数，使用 tiktoken 和 API 校准混合策略。

### 4.1 Token 基线结构体

```rust
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct TokenBaseline {
    pub last_known_input_tokens: u64,
    pub last_known_message_count: usize,
}
```

### 4.2 TokenCounter 结构体

```rust
pub struct TokenCounter {
    last_known_input_tokens: Option<u64>,
    last_known_message_count: usize,
    threshold_ratio: f64,
    context_window: u64,
}
```

### 4.3 核心方法

```rust
impl TokenCounter {
    pub fn new(context_window: u64, threshold_ratio: f64) -> Self;
    
    // 使用 tiktoken cl100k_base 计算 token 数
    pub fn count_text_tokens(text: &str) -> u64;
    
    // API 响应后校准
    pub fn calibrate(&mut self, api_input_tokens: u64, message_count_at_call: usize);
    
    // 估算当前 token 数
    pub fn estimate(&self, messages: &[Message]) -> u64;
    
    // 是否应该压缩
    pub fn should_compact(&self, messages: &[Message]) -> bool;
}
```

### 4.4 多级预算管理

```rust
impl TokenCounter {
    pub fn effective_window(&self) -> u64;
    pub fn warning_threshold(&self) -> u64;
    pub fn blocking_threshold(&self) -> u64;
    pub fn compact_threshold(&self) -> u64;
    pub fn budget_status(&self, messages: &[Message]) -> BudgetStatus;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetStatus {
    Normal,   // 正常
    Compact,  // 超过压缩阈值
    Warning,  // 接近限制
    Blocking, // 必须压缩
}
```

---

## 5. CostTracker - 成本追踪器

**源文件**: `cost_tracker.rs`

CostTracker 计算 API 调用成本并累计总费用。

### 5.1 模型定价结构体

```rust
#[derive(Clone, Debug)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_write_per_mtok: f64,
    pub cache_read_per_mtok: f64,
}
```

### 5.2 CostTracker 结构体

```rust
pub struct CostTracker {
    pricing: HashMap<String, ModelPricing>,
    current_query_cost: f64,
    total_cost: f64,
    total_usage: Usage,
}
```

### 5.3 内置模型定价

```rust
// Claude Sonnet 4: input=$3/M, output=$15/M
// Claude Opus 4: input=$15/M, output=$75/M
// Claude Haiku 3.5: input=$0.80/M, output=$4/M
```

### 5.4 核心方法

```rust
impl CostTracker {
    pub fn new() -> Self;
    pub fn calculate_cost(&self, usage: &Usage, model: &str) -> f64;
    pub fn accumulate(&mut self, usage: &Usage, model: &str);
    pub fn reset_query(&mut self);
    pub fn current_query_cost(&self) -> f64;
    pub fn total_cost(&self) -> f64;
}
```

---

## 6. TranscriptWriter - 会话转录

**源文件**: `transcript.rs`

TranscriptWriter 将会话事件持久化为 JSONL 文件，支持会话恢复和审计。

### 6.1 转录条目结构体

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TranscriptEntry {
    pub timestamp: String,
    pub entry_type: TranscriptEntryType,
    pub data: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TranscriptEntryType {
    UserMessage,
    AssistantMessage,
    ToolUse,
    ToolResult,
    SystemEvent,
}
```

### 6.2 TranscriptWriter 结构体

```rust
pub struct TranscriptWriter {
    file: std::fs::File,
    session_id: String,
}
```

### 6.3 核心方法

```rust
impl TranscriptWriter {
    // 打开会话转录文件 (~/.baoclaw/sessions/{session_id}.jsonl)
    pub fn open(session_id: &str) -> Result<Self, std::io::Error>;
    
    // 追加条目
    pub fn append(&mut self, entry: &TranscriptEntry) -> Result<(), std::io::Error>;
    
    // 加载所有条目
    pub fn load(session_id: &str) -> Result<Vec<TranscriptEntry>, std::io::Error>;
}

// 从转录重建消息
pub fn rebuild_messages_from_transcript(entries: &[TranscriptEntry]) -> Vec<Message>;

// 查找项目的最新会话
pub fn find_latest_session_for_cwd(cwd: &str) -> Option<String>;
```

---

## 7. SharedSession - 多客户端会话

**源文件**: `shared_session.rs`

SharedSession 包装 QueryEngine，支持多客户端并发访问同一会话。

### 7.1 SharedSession 结构体

```rust
pub type ClientId = u64;

pub struct SharedSession {
    engine: Arc<RwLock<QueryEngine>>,
    active_submitter: Mutex<Option<ClientId>>,
    event_tx: broadcast::Sender<EngineEvent>,
    connected_clients: Mutex<HashSet<ClientId>>,
    next_client_id: AtomicU64,
}
```

### 7.2 核心方法

```rust
impl SharedSession {
    pub fn new(engine: QueryEngine, broadcast_capacity: usize) -> Self;
    
    // 注册新客户端
    pub async fn add_client(&self) -> (ClientId, broadcast::Receiver<EngineEvent>);
    
    // 移除客户端
    pub async fn remove_client(&self, client_id: ClientId) -> bool;
    
    // 尝试获取提交锁
    pub async fn try_acquire_submitter(&self, client_id: ClientId) -> bool;
    
    // 读写锁访问
    pub async fn engine_read(&self) -> RwLockReadGuard<'_, QueryEngine>;
    pub async fn engine_write(&self) -> RwLockWriteGuard<'_, QueryEngine>;
    
    // 广播事件
    pub fn broadcast(&self, event: EngineEvent);
}
```

### 7.3 SessionRegistry - 全局会话注册表

```rust
pub struct SessionRegistry {
    sessions: Mutex<HashMap<String, Arc<SharedSession>>>,
}

impl SessionRegistry {
    pub fn new() -> Self;
    pub async fn get_or_create(
        &self,
        session_id: &str,
        config_factory: impl FnOnce() -> QueryEngine,
    ) -> (Arc<SharedSession>, bool);
}
```

---

## 8. EvolutionEngine - 自我进化引擎

**源文件**: `evolution.rs`

EvolutionEngine 从交互中学习，创建和改进技能。

### 8.1 核心常量

```rust
const SKILL_CREATION_THRESHOLD: usize = 3;  // 考虑任务"复杂"的最小工具调用数
const SELF_EVAL_INTERVAL: usize = 15;       // 每 N 个完成任务评估一次
```

### 8.2 会话摘要结构体

```rust
pub struct SessionSummary {
    pub session_id: String,
    pub timestamp: String,
    pub cwd: String,
    pub model: String,
    pub duration_secs: u64,
    pub turn_count: usize,
    pub user_topics: Vec<String>,
    pub tool_usage: Vec<(String, u32)>,
    pub errors: Vec<(String, String)>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub skills_used: Vec<String>,
}
```

### 8.3 轨迹结构体

```rust
pub struct Trajectory {
    pub id: String,
    pub timestamp: String,
    pub cwd: String,
    pub user_prompt: String,
    pub assistant_actions: Vec<TrajectoryAction>,
    pub outcome: TrajectoryOutcome,
    pub tool_count: usize,
    pub duration_ms: u64,
    pub user_rating: Option<TrajectoryRating>,
}

pub enum TrajectoryOutcome {
    Completed { final_text_preview: String },
    MaxTurns,
    Aborted,
    Error { code: String, message: String },
}
```

### 8.4 EvolutionEngine 结构体

```rust
pub struct EvolutionEngine {
    base_dir: Mutex<PathBuf>,
    task_count: Mutex<usize>,
    skills_dir: PathBuf,
    skill_stats: Mutex<HashMap<String, SkillStats>>,
    trajectories: Mutex<Vec<Trajectory>>,
}
```

### 8.5 核心方法

```rust
impl EvolutionEngine {
    pub fn new(_cwd: &Path) -> Self;
    pub async fn record_trajectory(&self, trajectory: Trajectory);
    pub async fn promote_skill(&self, _cwd: &Path, candidate_name: &str, skill_content: &str) -> Result<String, String>;
    pub async fn on_session_close(/* ... */);
    pub async fn export_training_data(&self) -> Vec<Value>;
    pub async fn build_prompt_fragment(&self, _cwd: &Path) -> Option<String>;
}
```

---

## 9. StreamingExecutor - 流式执行器

**源文件**: `streaming_executor.rs`

StreamingExecutor 提供长时间运行工具的实时进度报告。

### 9.1 流式块结构体

```rust
pub struct StreamChunk {
    pub execution_id: String,
    pub tool_name: String,
    pub chunk_type: StreamChunkType,
    pub content: String,
    pub seq: u32,
    pub timestamp: String,
}

pub enum StreamChunkType {
    Started,
    Progress,
    Stdout,
    Stderr,
    Completed,
    Error,
    Heartbeat,
}
```

### 9.2 流式配置

```rust
pub struct StreamingConfig {
    pub buffer_size: usize,
    pub heartbeat_interval_ms: u64,
    pub timeout_secs: u64,
    pub stream_stdout: bool,
    pub stream_stderr: bool,
    pub max_output_bytes: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            buffer_size: 256,
            heartbeat_interval_ms: 5000,
            timeout_secs: 300,
            stream_stdout: true,
            stream_stderr: true,
            max_output_bytes: 1024 * 1024, // 1MB
        }
    }
}
```

### 9.3 StreamWriter 和 StreamReader

```rust
pub struct StreamWriter {
    sender: mpsc::Sender<StreamChunk>,
    execution_id: String,
    seq_counter: u32,
}

pub struct StreamReader {
    receiver: mpsc::Receiver<StreamChunk>,
    execution_id: String,
}

pub fn create_stream_pair(execution_id: String) -> (StreamWriter, StreamReader);
```

---

## 10. SandboxConfig - 沙箱执行

**源文件**: `sandbox.rs`

SandboxConfig 提供工具执行的隔离环境。

### 10.1 沙箱后端类型

```rust
pub enum SandboxBackend {
    None,                                    // 无沙箱
    Bubblewrap,                              // Linux namespace 隔离
    Docker { image: String },                // Docker 容器隔离
}
```

### 10.2 沙箱配置结构体

```rust
pub struct SandboxConfig {
    pub backend: SandboxBackend,
    pub rw_mounts: Vec<String>,
    pub ro_mounts: Vec<String>,
    pub env_passthrough: Vec<String>,
    pub allow_network: bool,
    pub memory_limit_mb: u32,
    pub cpu_time_limit_secs: u32,
    pub workdir: Option<String>,
}
```

### 10.3 核心方法

```rust
impl SandboxConfig {
    pub fn auto_detect() -> Self;
    pub fn wrap_command(&self, command: &str, cwd: &Path) -> String;
    pub fn build_command_args(&self, command: &str, cwd: &Path) -> Vec<String>;
    pub fn is_available(&self) -> bool;
    pub fn validate(&self) -> Option<String>;
    pub fn description(&self) -> &str;
}
```

---

## 11. Tool Trait - 工具系统

**源文件**: `tools/trait_def.rs`

Tool trait 定义了所有工具必须实现的核心接口。

### 11.1 工具执行结果

```rust
pub struct ToolResult {
    pub data: Value,
    pub is_error: bool,
}

pub enum ValidationResult {
    Ok,
    Invalid { message: String, code: Option<String> },
}

pub enum ToolPermissionCheckResult {
    Allow { updated_input: Value },
    Ask { message: String, updated_input: Value },
    Deny { message: String },
}
```

### 11.2 进度发送器 trait

```rust
#[async_trait]
pub trait ProgressSender: Send + Sync {
    async fn send_progress(&self, tool_use_id: &str, data: Value);
}
```

### 11.3 JSON Schema 结构体

```rust
pub struct JsonSchema {
    pub schema_type: String,
    pub properties: Option<Value>,
    pub required: Option<Vec<String>>,
    pub description: Option<String>,
}
```

### 11.4 Tool Trait 定义

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn aliases(&self) -> Vec<&str> { vec![] }
    fn input_schema(&self) -> JsonSchema;
    
    // 行为标记
    fn is_read_only(&self, _input: &Value) -> bool { false }
    fn is_destructive(&self, _input: &Value) -> bool { false }
    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }
    fn is_enabled(&self) -> bool { true }
    fn is_deferred(&self) -> bool { false }
    
    // 结果大小限制
    fn max_result_size_chars(&self) -> usize { 100_000 }
    
    // 核心执行方法
    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError>;
    
    // 验证和权限
    async fn validate_input(&self, _input: &Value, _context: &ToolContext) -> ValidationResult;
    async fn check_permissions(&self, _input: &Value, _context: &ToolContext) -> ToolPermissionCheckResult;
    
    // 提示生成
    fn prompt(&self) -> String;
    fn short_description(&self) -> String;
}
```

### 11.5 工具上下文

```rust
pub struct ToolContext {
    pub cwd: PathBuf,
    pub model: String,
    pub abort_signal: Arc<watch::Receiver<bool>>,
    pub file_cache: Option<Arc<Mutex<FileCache>>>,
    pub tool_result_store: Option<Arc<ToolResultStore>>,
}
```

### 11.6 工具错误类型

```rust
pub enum ToolError {
    ExecutionFailed(String),
    Timeout(u64),
    Aborted,
    Io(std::io::Error),
    Serialization(serde_json::Error),
}
```

### 11.7 工具执行器

```rust
// 执行单个工具
pub async fn execute_tool(
    tool: &dyn Tool,
    request: &ToolUseRequest,
    context: &ToolContext,
    progress: &dyn ProgressSender,
) -> ToolExecutionResult;

// 执行多个工具（并发安全工具并行执行）
pub async fn execute_tools(
    tools: &[Arc<dyn Tool>],
    requests: &[ToolUseRequest],
    context: &ToolContext,
    progress: &dyn ProgressSender,
) -> Vec<ToolExecutionResult>;
```

---

## 12. IPC 协议 - 进程间通信

**源文件**: `ipc/protocol.rs`, `ipc/server.rs`

IPC 层实现 JSON-RPC 2.0 over Unix Domain Socket 的通信协议。

### 12.1 请求 ID 类型

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}
```

### 12.2 JSON-RPC 消息类型

```rust
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
    pub id: RequestId,
}

pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Value,
    pub id: RequestId,
}

pub struct JsonRpcErrorResponse {
    pub jsonrpc: String,
    pub error: JsonRpcError,
    pub id: Option<RequestId>,
}

pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}
```

### 12.3 统一消息类型

```rust
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    ErrorResponse(JsonRpcErrorResponse),
    Notification(JsonRpcNotification),
}
```

### 12.4 NDJSON 编解码

```rust
pub fn encode_ndjson(message: &impl Serialize) -> Result<Vec<u8>, serde_json::Error>;
pub fn decode_ndjson_line(line: &str) -> Result<JsonRpcMessage, serde_json::Error>;
```

### 12.5 IPC Server

```rust
pub struct IpcServer {
    listener: UnixListener,
    socket_path: PathBuf,
}

pub struct IpcConnection {
    reader: BufReader<OwnedReadHalf>,
    writer: BufWriter<OwnedWriteHalf>,
}

impl IpcServer {
    // 绑定 Unix Domain Socket，权限设为 0600
    pub async fn bind(socket_path: &Path) -> std::io::Result<Self>;
    pub async fn accept(&self) -> std::io::Result<IpcConnection>;
}

impl IpcConnection {
    pub async fn send_response(&mut self, id: RequestId, result: Value) -> std::io::Result<()>;
    pub async fn send_error(&mut self, id: Option<RequestId>, code: i32, message: String) -> std::io::Result<()>;
    pub async fn send_notification(&mut self, method: &str, params: Value) -> std::io::Result<()>;
    pub async fn recv_message(&mut self) -> Result<JsonRpcMessage, IpcError>;
}
```

---

## 附录：模块依赖关系

```
engine/mod.rs (模块入口)
├── query_engine.rs    ← 核心引擎
├── memory.rs          ← 长期记忆
├── session_memory.rs  ← 会话记忆
├── token_counter.rs   ← Token 计数
├── cost_tracker.rs    ← 成本追踪
├── transcript.rs      ← 会话转录
├── shared_session.rs  ← 多客户端会话
├── evolution.rs       ← 自我进化
├── streaming_executor.rs ← 流式执行
├── sandbox.rs         ← 沙箱配置
├── error_handling.rs  ← 错误处理
└── ...

tools/
├── trait_def.rs       ← Tool trait 定义
├── executor.rs        ← 工具执行器
└── builtins/          ← 内置工具实现

ipc/
├── protocol.rs        ← JSON-RPC 协议
├── server.rs          ← IPC 服务器
├── router.rs          ← 路由处理
└── events.rs          ← 事件定义
```
