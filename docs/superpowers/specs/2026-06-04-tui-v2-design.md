# TUI v2.0 — Kiro 风格增强设计

**日期**: 2026-06-04  
**状态**: 方案已确认  
**参考**: https://kiro.dev/docs/cli/

## 1. 目标

参考 Kiro CLI 的交互设计，对现有 `ts-ipc/tui` 做全面升级：

| 模块          | 状态 | 说明                                 |
| ------------- | ---- | ------------------------------------ |
| 快捷键栏      | 新增 | `ShortcutBar.tsx`，底部常驻          |
| 斜杠命令补全  | 改造 | `InputArea.tsx` 新增候选弹窗         |
| 消息搜索      | 新增 | `SearchOverlay.tsx`，`Ctrl+R` 触发   |
| 模型切换      | 新增 | `ModelSelector.tsx`，`/model` 触发   |
| Markdown 增强 | 改造 | `MessageList.tsx` + 新 `markdown.ts` |
| 思考进度条    | 改造 | `StreamOutput.tsx` 加计时+动画       |
| 状态栏增强    | 改造 | `StatusBar.tsx` 加版本号             |
| 帮助覆盖层    | 改造 | `HelpOverlay.tsx` 更新快捷键说明     |

## 2. 架构

```
ts-ipc/tui/
├── components/
│   ├── App.tsx           [改] 布局改为 状态栏+消息+输入+快捷键四区
│   ├── ShortcutBar.tsx   [新] 底部常驻快捷键条
│   ├── StatusBar.tsx     [改] 加入版本号
│   ├── InputArea.tsx     [改] 斜杠命令自动补全（候选弹窗+键盘导航）
│   ├── MessageList.tsx   [改] Markdown 语法高亮 + 消息气泡分隔
│   ├── ModelSelector.tsx [新] 模型切换弹层（↑↓选择）
│   ├── SearchOverlay.tsx [新] Ctrl+R 消息搜索（实时过滤+高亮）
│   ├── HelpOverlay.tsx   [改] 更新快捷键说明
│   ├── StreamOutput.tsx  [改] 思考进度条（计时+动画帧）
│   └── ToolsPanel.tsx    [保持]
├── markdown.ts           [新] Markdown 语法高亮解析器
├── state.ts              [改] 新增 modelList/search/suggestions/version 状态
├── types.ts              [改] 新增类型定义
├── theme.ts              [改] 新增 Markdown 高亮色
└── index.tsx             [保持]
```

## 3. 布局

```
┌──────────────────────────────────────────┐
│  ● Session-abc · Sonnet 4.5 · 45% $0.002 │  状态栏（1行）
├──────────────────────────────────────────┤
│  ┌─ Messages ───────────────────────────┐│
│  │                                      ││
│  │  [You]  你好                         ││  主消息区（flexGrow）
│  │                                      ││  - 消息气泡分隔
│  │  [✦]  你好！有什么可以帮你？           ││  - 代码块语法高亮
│  │  ──────── · ────────                 ││  - 思考进度条内嵌
│  │  ○ thinking... (1.2s) ▓▓▓▓▓▓░░░░    ││
│  │                                      ││
│  └──────────────────────────────────────┘│
├──────────────────────────────────────────┤
│  ❯ /sta                                  │  输入区（3行）+ 候选弹窗
│  ┌──────────┐                            │
│  │ /status  │  ← ↑↓选择 Tab/Enter补全     │
│  │ /start   │                            │
│  └──────────┘                            │
├──────────────────────────────────────────┤
│  Enter:Send ↑↓:History Ctrl+R:Search ?:Help Esc:Close │ 快捷键栏（1行）
└──────────────────────────────────────────┘
```

## 4. 模块详设

### 4.1 快捷键栏 (`ShortcutBar.tsx`)

- 位置：底部固定 1 行
- 内容：`Enter:Send  ↑↓:History  Ctrl+R:搜索  ?:Help  Esc:Close`
- 实现：`<Box>` 水平排列，灰色文字，各键名用亮色高亮

### 4.2 斜杠命令自动补全 (`InputArea.tsx` 改造)

- 触发：输入 `/` 后弹出候选列表
- 过滤：实时 prefix 匹配 `state.commands` 注册表
- 注册表（13 个）：`/help /status /model /clear /compact /sessions /tools /memory /cron /git /search /skills /gateway`
- 键盘：
  - `↑↓` 在候选列表中移动
  - `Tab` / `Enter` 补全选中项
  - `Esc` 关闭候选
- UI：`<Box>` 绝对定位在输入行上方，高亮选中项

### 4.3 消息搜索 (`SearchOverlay.tsx`)

- 触发：`Ctrl+R`
- 关闭：`Esc`
- 布局：全屏覆盖层
- 功能：
  - 输入框实时过滤 `state.messages`
  - 匹配文本用反转色高亮
  - `↑↓` 浏览结果
  - `Enter` 跳转到对应消息（滚动定位）
  - 显示 `[2/5]` 当前位置/总数

### 4.4 模型切换 (`ModelSelector.tsx`)

- 触发：`/model`（弹选择器）或 `/model sonnet`（直接切换）
- 模型列表：从 daemon `listModels` RPC 获取
- UI：`<Box>` 列表弹层，当前模型标记 `⬤`，其余 `○`
- 键盘：`↑↓` 选择，`Enter` 确认，`Esc` 关闭
- 动作：确认后调 daemon `setModel` RPC，更新状态栏

### 4.5 Markdown 增强 (`markdown.ts` + `MessageList.tsx`)

- 代码块语法高亮：
  - 解析 ` ```language ``` ` 块
  - 关键字染色：粉红色（`function/const/let/import/export/class/return/if/else/for/while`）
  - 字符串染色：黄色（`"..."` / `'...'` / `` `...` ``）
  - 注释染色：灰色（`//` / `/* */`）
  - 函数名染色：绿色
  - 类型染色：青色
- 消息分隔：每条消息之间加 `──── · ────` 淡色分隔线
- 角色前缀：`You`（橙色） / `✦`（青色）

### 4.6 思考进度条 (`StreamOutput.tsx` 改造)

- 原有 `○ thinking...` 替换为：
  - `○ thinking... (2.3s) ▓▓▓▓▓▓▓░░░ 73%`
- 计时器：从 thought 开始计时，实时显示 elapsed
- 进度条：线性增长 0→95%（10 秒满），超过 10 秒维持 95%
- 实现：`setInterval` 每 100ms 更新，thought 结束时清空

### 4.7 状态栏增强 (`StatusBar.tsx`)

- 原 `● Session-abc · Sonnet · 45% · $0.002`
- 新增：版本号 `v2.0` 在右侧末尾

## 5. 状态扩展 (`state.ts` / `types.ts`)

```ts
// 新增状态字段
interface TuiState {
  // ...existing...
  modelList: string[];          // 可用模型列表
  currentModel: string;         // 当前模型
  showModelSelector: boolean;   // 模型选择器可见
  suggestions: string[];        // 自动补全候选
  selectedSuggestion: number;   // 候选选中索引
  showSuggestions: boolean;     // 候选框可见
  searchQuery: string;          // 搜索关键词
  searchResults: number[];      // 匹配消息索引
  selectedSearchResult: number; // 搜索结果选中
  version: string;              // TUI 版本号
}

// 新增 Action
type ActionType =
  | ...existing...
  | 'SET_MODEL_LIST'
  | 'SET_CURRENT_MODEL'
  | 'SHOW_MODEL_SELECTOR'
  | 'SET_SUGGESTIONS'
  | 'SET_SELECTED_SUGGESTION'
  | 'SHOW_SUGGESTIONS'
  | 'SET_SEARCH_QUERY'
  | 'SET_SEARCH_RESULTS'
  | 'SET_SELECTED_SEARCH_RESULT';
```

## 6. 颜色扩展 (`theme.ts`)

```ts
markdown: {
  codeBg: '#1A1A1A',
  keyword: '#FF79C6',  // 粉红
  string: '#F1FA8C',   // 黄
  comment: '#6272A4',  // 灰蓝
  fn: '#50FA7B',       // 绿
  type: '#8BE9FD',     // 青
  number: '#BD93F9',   // 紫
}
```

## 7. 命令注册表

| 命令            | 说明       | 参数       |
| --------------- | ---------- | ---------- |
| `/help`         | 显示帮助   | -          |
| `/status`       | 会话状态   | -          |
| `/model [name]` | 切换模型   | 可选模型名 |
| `/clear`        | 清屏       | -          |
| `/compact`      | 压缩上下文 | -          |
| `/sessions`     | 会话列表   | -          |
| `/tools`        | 工具面板   | -          |
| `/memory`       | 记忆管理   | -          |
| `/cron`         | 定时任务   | -          |
| `/git`          | Git 状态   | -          |
| `/search <q>`   | 搜索消息   | 关键词     |
| `/skills`       | 技能列表   | -          |
| `/gateway`      | 网关状态   | -          |

## 8. 文件改动清单

| 文件                           | 操作 | 预估行数 |
| ------------------------------ | ---- | -------- |
| `components/ShortcutBar.tsx`   | 新建 | ~40      |
| `components/ModelSelector.tsx` | 新建 | ~80      |
| `components/SearchOverlay.tsx` | 新建 | ~120     |
| `markdown.ts`                  | 新建 | ~150     |
| `components/App.tsx`           | 改造 | +15      |
| `components/InputArea.tsx`     | 改造 | +60      |
| `components/MessageList.tsx`   | 改造 | +40      |
| `components/StatusBar.tsx`     | 改造 | +5       |
| `components/StreamOutput.tsx`  | 改造 | +30      |
| `components/HelpOverlay.tsx`   | 改造 | +10      |
| `state.ts`                     | 改造 | +50      |
| `types.ts`                     | 改造 | +30      |
| `theme.ts`                     | 改造 | +10      |

总计：~640 新增行，4 个新文件，9 个改造文件。
