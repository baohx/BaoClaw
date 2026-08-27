# BaoClaw 权限系统 (Permission System)

本文档描述 BaoClaw 的工具执行权限控制机制，包括架构、数据结构、检查流程和配置方式。

## 目录

1. [架构概览](#1-架构概览)
2. [核心数据结构](#2-核心数据结构)
3. [PermissionManager — 规则匹配引擎](#3-permissionmanager--规则匹配引擎)
4. [PermissionGate — 异步决策通道](#4-permissiongate--异步决策通道)
5. [ToolExecutor — 执行流水线](#5-toolexecutor--执行流水线)
6. [RuleBasedPermissionGate — 引擎级规则缓存](#6-rulebasedpermissiongate--引擎级规则缓存)
7. [Security 模块 — 危险命令阻断](#7-security-模块--危险命令阻断)
8. [权限检查完整流程图](#8-权限检查完整流程图)
9. [配置方式](#9-配置方式)

---

## 1. 架构概览

BaoClaw 权限系统由以下组件组成（分层设计）：

```
┌────────────────────────────────────────────────────────────────────┐
│                         CLI (ts-ipc/cli.ts)                         │
│                     斜杠命令 /permission, /permissions              │
│                         用户交互层                                    │
└───────────────┬────────────────────────────────────┬───────────────┘
                │ JSON-RPC                            │ PermissionRequest
                │ 事件                                 │ 事件 (EngineEvent)
┌───────────────▼────────────────────────────────────▼───────────────┐
│                      Daemon (main.rs)                               │
│  ┌─────────────────────┐  ┌──────────────────────────────────────┐ │
│  │   IPC Router        │  │   QueryEngine                         │ │
│  │   ClientMethod 枚举  │  │   ┌──────────────────────────────┐   │ │
│  │   - PermissionStatus│  │   │  ToolExecutor                 │   │ │
│  │   - PermissionGrant │  │   │  execute_tool_with_permission │   │ │
│  │   - PermissionRevoke│  │   └──────────┬───────────────────┘   │ │
│  └─────────────────────┘  │              │                        │ │
│                           │  ┌───────────▼────────────┐           │ │
│                           │  │ PermissionManager      │           │ │
│                           │  │ (rules + glob match)   │           │ │
│                           │  └────────────────────────┘           │ │
│                           │  ┌────────────────────────┐           │ │
│                           │  │ PermissionGate         │           │ │
│                           │  │ (oneshot channels)     │           │ │
│                           │  └────────────────────────┘           │ │
│                           └──────────────────────────────────────┘ │
│  ┌─────────────────────────────────────────────────────────────┐  │
│  │  RuleBasedPermissionGate (engine/permission_gate)           │  │
│  │  - 内置安全规则 (deny rm -rf, sudo, etc.)                   │  │
│  │  - 用户缓存授权 (AllowSession / AllowPermanent)             │  │
│  └─────────────────────────────────────────────────────────────┘  │
│  ┌─────────────────────────────────────────────────────────────┐  │
│  │  Security 模块 (engine/security.rs)                         │  │
│  │  - check_dangerous_command() 硬阻断                          │  │
│  │  - check_ssrf_url() SSRF 防护                                │  │
│  │  - validate_memory_content() 凭据泄漏检测                     │  │
│  └─────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────┘
```

### 文件清单

| 文件                                              | 职责                                                                         |
| ------------------------------------------------- | ---------------------------------------------------------------------------- |
| `baoclaw-core/src/permissions/manager.rs`         | `PermissionManager` + `PermissionMode` + `PermissionRule` + glob 匹配        |
| `baoclaw-core/src/permissions/gate.rs`            | `PermissionGate` (pending 请求队列 + oneshot channel) + `PermissionDecision` |
| `baoclaw-core/src/tools/executor.rs`              | `ToolExecutor` — 工具执行流水线，调用 PermissionManager + PermissionGate     |
| `baoclaw-core/src/engine/permission_gate/gate.rs` | `RuleBasedPermissionGate` — 引擎级规则策略 + 缓存                            |
| `baoclaw-core/src/engine/security.rs`             | 危险命令阻断、SSRF 防护、内容验证                                            |

---

## 2. 核心数据结构

### PermissionMode

```rust
pub enum PermissionMode {
    Default,            // 默认模式：不匹配 allow 规则的工具 → Ask
    Plan,               // 计划模式：只读工具自动 Allow，其他 Ask
    BypassPermissions,  // 绕过模式：所有非 deny 工具自动 Allow
    Auto,               // 自动模式（预留）
}
```

| 模式              | 读操作                        | 写操作 | deny 规则   |
| ----------------- | ----------------------------- | ------ | ----------- |
| Default           | Ask (除非 allow 规则匹配)     | Ask    | ✅ 强制阻断 |
| Plan              | Allow (Read/Grep/Glob/Search) | Ask    | ✅ 强制阻断 |
| BypassPermissions | Allow                         | Allow  | ✅ 强制阻断 |
| Auto              | (预留)                        | (预留) | ✅ 强制阻断 |

### PermissionRule

```rust
pub struct PermissionRule {
    pub tool_name: String,        // 工具名（大小写不敏感）
    pub rule_content: Option<String>,  // glob 模式，如 "rm -rf *"；None = 匹配所有输入
}
```

### ToolPermissionContext

```rust
pub struct ToolPermissionContext {
    pub mode: PermissionMode,
    pub additional_working_directories: HashMap<String, String>,
    pub always_allow_rules: ToolPermissionRulesBySource,  // HashMap<source, Vec<PermissionRule>>
    pub always_deny_rules: ToolPermissionRulesBySource,
    pub always_ask_rules: ToolPermissionRulesBySource,
    pub is_bypass_permissions_mode_available: bool,
}
```

**规则按 source 分组**，`source` 可以是 `"builtin"`、`"user"`、`"config"` 等。检查时遍历所有 source 的规则。

### PermissionResult

```rust
pub enum PermissionResult {
    Allow,
    Ask { message: String },
    Deny { message: String },
}
```

---

## 3. PermissionManager — 规则匹配引擎

**文件**: `baoclaw-core/src/permissions/manager.rs`

### check_permission() 检查顺序

`check_permission(tool_name, input_description)` 按以下顺序求值（短路返回）：

```
Step 1: deny 规则检查（最高优先级）
  → 匹配 → return Deny

Step 2: BypassPermissions 模式
  → return Allow (跳过后续所有检查)

Step 3: allow 规则检查
  → 匹配 → return Allow

Step 4: ask 规则检查
  → 匹配 → return Ask

Step 5: Plan 模式
  → 只读工具 (Read/Grep/Glob/Search) → Allow
  → 其他 → Ask

Step 6: 默认 → Ask
```

**关键设计**: deny 规则始终最先检查，确保即使用户设了 BypassPermissions 模式，危险操作仍被阻断。

### glob_matches() — 通配符匹配

使用动态规划实现 `*` 通配符匹配：

- `*` 匹配任意长度字符序列（包括空串）
- 匹配是大小写不敏感的
- 示例：
  - `"git *"` 匹配 `"git push origin main"`
  - `"rm -rf *"` 匹配 `"rm -rf /tmp/build"`
  - `"*"` 匹配任何字符串

### matches_rule() — 单条规则匹配

```rust
fn matches_rule(rule: &PermissionRule, tool_name: &str, input_description: Option<&str>) -> bool {
    // 1. 工具名大小写不敏感匹配
    if !rule.tool_name.eq_ignore_ascii_case(tool_name) { return false; }

    // 2. 如果规则有 content 模式，输入描述必须匹配 glob
    match (&rule.rule_content, input_description) {
        (Some(pattern), Some(desc)) => glob_matches(pattern, desc),
        (Some(_), None) => false,  // 有模式但无输入描述 → 不匹配
        (None, _) => true,          // 无模式 → 匹配任意输入
    }
}
```

### API 方法

| 方法                                                          | 说明             |
| ------------------------------------------------------------- | ---------------- |
| `new(context: ToolPermissionContext)`                         | 创建 manager     |
| `check_permission(tool_name, input_desc) -> PermissionResult` | 核心检查方法     |
| `update_context(FnOnce(&mut ctx))`                            | 用闭包更新上下文 |
| `get_context() -> ToolPermissionContext`                      | 获取上下文快照   |
| `add_allow_always_rule(source, tool_name, rule_content)`      | 添加 allow 规则  |

---

## 4. PermissionGate — 异步决策通道

**文件**: `baoclaw-core/src/permissions/gate.rs`

`PermissionGate` 是 CLI ↔ Daemon 之间的异步通信桥梁，用于处理需要用户确认的权限请求。

### 工作机制

```
Daemon (ToolExecutor)                     CLI
    │                                       │
    │  PermissionGate.request(tool_use_id)   │
    │  → 返回 oneshot::Receiver              │
    │  → 阻塞等待...                          │
    │                           ◄──────────│  EngineEvent::PermissionRequest
    │                           (IPC event) │  显示确认提示给用户
    │                                       │
    │                           ◄──────────│  PermissionResponse { decision }
    │  PermissionGate.respond(tool_use_id,  │  (allow/deny/allow_always)
    │    decision)                           │
    │  → oneshot::Sender.send(decision)      │
    │  ← oneshot::Receiver 收到 decision     │
    │  → 继续执行或拒绝                        │
```

### PermissionDecision

```rust
pub enum PermissionDecision {
    Allow,                  // 本次允许
    Deny,                   // 拒绝
    AllowAlways {           // 永久允许（添加到 allow 规则）
        rule: Option<String>,
    },
}
```

### 超时机制

`ToolExecutor` 在 `execute_tool_with_permission` 中使用 **5 分钟超时**：

```rust
let decision = match tokio::time::timeout(Duration::from_secs(300), rx).await {
    Ok(Ok(decision)) => decision,
    Ok(Err(_)) => PermissionDecision::Deny,  // channel 关闭 → 拒绝
    Err(_) => PermissionDecision::Deny,      // 超时 → 自动拒绝
};
```

### API 方法

| 方法                                                   | 说明                                |
| ------------------------------------------------------ | ----------------------------------- |
| `new()`                                                | 创建空 gate                         |
| `request(tool_use_id) -> Receiver<PermissionDecision>` | 注册 pending 请求，返回等待 channel |
| `respond(tool_use_id, decision) -> bool`               | 提交用户决策，返回是否成功投递      |
| `pending_count() -> usize`                             | 当前 pending 请求数                 |

---

## 5. ToolExecutor — 执行流水线

**文件**: `baoclaw-core/src/tools/executor.rs`

### execute_tool_with_permission()

这是核心工具执行函数，集成了 PermissionManager + PermissionGate：

```
┌─────────────────────────────────────────────────────┐
│  Step 1: validate_input(&request.input)             │
│  → Invalid → 返回错误                                │
├─────────────────────────────────────────────────────┤
│  Step 2: permission_manager.check_permission(       │
│            tool_name, input_description)             │
│                                                      │
│  → Allow  → 直接执行 (call_tool_and_wrap)            │
│  → Deny   → 返回 "Permission denied"                │
│  → Ask    → 进入交互式确认流程 ──┐                   │
├───────────────────────────────◄──┘                   │
│  Step 3 (Ask 分支):                                  │
│  a. 发送 EngineEvent::PermissionRequest              │
│  b. PermissionGate.request(tool_use_id)              │
│  c. 等待 5 分钟超时                                   │
│                                                      │
│  → Allow          → 执行                            │
│  → AllowAlways    → 添加规则 + 执行                  │
│  → Deny           → 返回 "Permission denied by user"│
├─────────────────────────────────────────────────────┤
│  Step 4: tool.call(input, context, progress)        │
│  → maybe_persist_or_truncate(result)                │
└─────────────────────────────────────────────────────┘
```

### 两条执行路径

| 函数                             | 用途                                    | 权限检查方式                             |
| -------------------------------- | --------------------------------------- | ---------------------------------------- |
| `execute_tool()`                 | 简单路径（批量执行）                    | 只检查 Tool trait 的 `check_permissions` |
| `execute_tool_with_permission()` | 完整路径（带 PermissionManager + Gate） | PermissionManager → Gate 交互式          |

---

## 6. RuleBasedPermissionGate — 引擎级规则缓存

**文件**: `baoclaw-core/src/engine/permission_gate/gate.rs`

这是一个独立的权限策略引擎，用于 QueryEngine 层面的规则管理和会话缓存。

### 内置默认规则

| 工具                  | 模式                                                             | 策略                    |
| --------------------- | ---------------------------------------------------------------- | ----------------------- |
| FileRead              | `*`                                                              | ✅ Always Allow         |
| FileWrite             | `*.env`, `.git/*`, `*/.ssh/*`                                    | ❌ Auto Deny            |
| FileWrite             | `*.md`                                                           | ✅ Allow                |
| FileWrite             | `*` (其他)                                                       | ❓ Require Confirmation |
| Bash                  | `rm -rf /`, `sudo `, `chmod 777`, `dd if=`, `mkfs.`, `> /dev/sd` | ❌ Auto Deny            |
| Bash                  | `git status`, `git diff`, `ls `, `cat `, `grep `, `find `, `pwd` | ✅ Allow                |
| Bash                  | `*` (其他)                                                       | ❓ Require Confirmation |
| FileDelete / FileEdit | `*`                                                              | ❓ Require Confirmation |
| WebFetch              | `localhost:*`, `127.*`, `10.*`                                   | ❌ Auto Deny            |
| WebFetch              | `*` (外部)                                                       | ❓ Require Confirmation |
| WebSearch             | `*`                                                              | ✅ Allow                |

### 缓存决策类型

```rust
pub enum DecisionType {
    AllowOnce,        // 只本次（不缓存）
    AllowSession,     // 会话内有效（默认 TTL 24h）
    AllowPermanent,   // 永久
    Deny,
    AskUser,
}
```

### 评估顺序

1. **检查缓存** — 如果有 AllowSession/AllowPermanent 的缓存授权 → 直接返回
2. **按顺序匹配规则** — 第一个匹配的规则生效
3. **默认** → AskUser

---

## 7. Security 模块 — 危险命令阻断

**文件**: `baoclaw-core/src/engine/security.rs`

Security 模块提供了三层额外的安全防护（独立于 PermissionManager）：

### 7.1 危险命令阻断 — `check_dangerous_command()`

硬编码的危险命令黑名单（子串匹配，大小写不敏感）：

- `rm -rf /*` / `rm -rf /` — 递归根删除
- `:(){ :|:& };:` — fork bomb
- `dd if=` / `of=/dev/sd*` — 块设备写入
- `mkfs` — 文件系统格式化
- `chmod 777 /` / `chmod -r 777 /` — 根目录全局可写
- `> /etc/passwd` / `> /etc/shadow` — 覆盖认证文件
- `shutdown` / `reboot` / `poweroff` / `halt` — 系统电源操作
- `> /dev/sda*` / `> /dev/nvme*` — 直接写块设备

### 7.2 SSRF 防护 — `check_ssrf_url()`

阻断指向内部/私有网络的 URL：

- `127.0.0.0/8` — Loopback
- `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16` — RFC 1918
- `169.254.0.0/16` — Link-local (含 `169.254.169.254` 云元数据)
- `100.64.0.0/10` — CGNAT
- `::1`, `fc00::/7`, `fe80::/10` — IPv6 私有
- `metadata.google.internal`, `metadata.internal` — 云元数据端点

### 7.3 内存内容验证 — `validate_memory_content()`

检测以下内容并拒绝写入长期记忆：

- **凭据泄漏**: `sk-*`, `ghp_*`, `AKIA*`, `xox[bpas]-*`, `Bearer *`
- **不可见 Unicode**: 零宽空格 `\u200B`、BOM `\uFEFF`、RTL 覆盖 `\u202E` 等
- **提示注入**: "ignore previous instructions" 等短语

---

## 8. 权限检查完整流程图

```
用户发送消息 → LLM 返回 tool_use
       │
       ▼
┌──────────────────────────┐
│ ToolExecutor             │
│ .execute_tool_with_perm  │
└──────────┬───────────────┘
           │
     ┌─────▼─────┐
     │ validate  │──invalid──► return error
     └─────┬─────┘
           │ ok
     ┌─────▼──────────────────┐
     │ PermissionManager      │
     │ .check_permission()    │
     └─────┬──────┬──────┬────┘
           │      │      │
      Allow│   Ask│   Deny│
           │      │      │
           │      │  ┌───▼──────────────┐
           │      │  │ return "Denied"  │
           │      │  └──────────────────┘
           │  ┌───▼──────────────────────┐
           │  │ EngineEvent::Permission  │
           │  │ Request → CLI             │
           │  ├──────────────────────────┤
           │  │ PermissionGate.request()  │
           │  │ 等待 5 分钟               │
           │  └───┬──────────┬──────┬─────┘
           │      │          │      │
           │   Allow    AllowAlways Deny
           │      │          │      │
           │      │    ┌─────▼──────────┐
           │      │    │ add_allow_rule │
           │      │    └─────┬──────────┘
           │      │          │
     ┌─────▼──────▼──────────▼──┐
     │ call_tool_and_wrap()      │
     │  tool.call(input, ctx)   │
     │  → maybe_persist/truncate│
     └──────────────────────────┘
```

---

## 9. 配置方式

### ~/.baoclaw/config.json 中的 permissions 字段

```json
{
  "permissions": {
    "mode": "default",
    "additional_working_directories": {},
    "always_allow_rules": {
      "builtin": [
        { "tool_name": "Read", "rule_content": null },
        { "tool_name": "Bash", "rule_content": "git status" },
        { "tool_name": "Bash", "rule_content": "git diff *" }
      ]
    },
    "always_deny_rules": {
      "builtin": [
        { "tool_name": "Bash", "rule_content": "rm -rf *" },
        { "tool_name": "Bash", "rule_content": "sudo *" }
      ]
    },
    "always_ask_rules": {
      "builtin": [{ "tool_name": "Bash", "rule_content": "*" }]
    }
  }
}
```

### 运行时修改

通过 IPC 方法或斜杠命令：

- `/permission status` — 查看引擎级规则状态
- `/permission grant <tool> <action> <target> [--permanent]` — 授权
- `/permission revoke <tool> <action> <target>` — 撤销
- `/permissions` — 查看 PermissionManager 上下文（mode + 三类规则）
- `/permissions mode <default|plan|bypass|auto>` — 切换权限模式
- `/permissions allow <tool> [glob]` — 添加 allow 规则
- `/permissions deny <tool> [glob]` — 添加 deny 规则
- `/permissions ask <tool> [glob]` — 添加 ask 规则

### 向后兼容

- `config.json` 中 `permissions` 字段不存在时，使用默认值（mode=Default, 空规则）
- `BaoclawConfig` 通过 `#[serde(flatten)] extra` 保留未知字段，确保前向兼容
