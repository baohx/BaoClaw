# Agent Harness 实战：从 BaoClaw 看 AI Agent 系统架构

> 以一个真实的 Agent Harness 产品为原型，深入剖析 AI Agent 系统的设计模式与工程实践。
>
> 🐾 **BaoClaw 开源地址：[github.com/baohx/BaoClaw](https://github.com/baohx/BaoClaw)** (MIT License)

## 关于本书

本书是一本采用 HTML 幻灯片风格呈现的 Agent Harness 技术书籍，以 BaoClaw 项目为参考实现，系统讲解 Agent 运行时框架的原理与实现。本书以 Rust 为主要技术语言，整合了真实的工程代码示例，注重实用性和可操作性。

与市面上大多数 Agent 教程不同，本书不是 API 调用指南或 Prompt 技巧集锦，而是从工程角度回答：

- 一个 Agent 系统的核心循环是什么？
- 工具系统如何设计才能可扩展？
- 多客户端如何共享同一个 Agent 会话？
- 记忆系统如何跨会话持久化？
- MCP 协议如何让 Agent 获得无限能力？
- 生产环境中如何处理上下文窗口、错误恢复、并发控制？

每一个问题，都有 BaoClaw 的真实代码作为答案。

## 目标读者

本书假设读者具备以下背景：

- Rust 或 TypeScript 基础编程能力
- 对 LLM API 有基本了解
- 对 Agent 概略有认知

本书聚焦 Agent Harness 特有技术，不过度解释基础编程概念。如需补充前置知识，可参考以下资源：

- [Rust 程序设计语言](https://doc.rust-lang.org/book/)
- [TypeScript 官方文档](https://www.typescriptlang.org/docs/)
- [OpenAI API 文档](https://platform.openai.com/docs/)

## 参考实现：BaoClaw

BaoClaw 是一个完整的 Agent Harness 系统，包含：

| 组件 | 技术栈 | 职责 |
|------|--------|------|
| baoclaw-core | Rust | 全局守护进程、多项目 Session 管理、QueryEngine、工具执行、IPC 服务、Cron 调度、进化引擎 |
| ts-ipc | TypeScript | 终端 CLI 客户端（按 cwd 自动路由到对应项目 session） |
| baoclaw-telegram | TypeScript | Telegram 网关客户端（支持 /cd 切换项目） |
| baoclaw-whatsapp | TypeScript | WhatsApp 网关客户端 |
| baoclaw-feishu | TypeScript | 飞书网关客户端 |

**GitHub**: [github.com/baohx/BaoClaw](https://github.com/baohx/BaoClaw)

架构特点：**一个全局 daemon 管理所有项目**。每个项目目录对应独立的 session（独立对话历史、记忆、配置）。多个 CLI 终端和消息平台可同时连接，按工作目录自动路由到正确的 session，互不干扰。

核心特性：

- 持久记忆：跨会话的知识存储与检索
- 全局守护进程：多项目会话管理
- 自我进化引擎：Skill 自动生成 + RLHF 数据导出
- Cron 定时任务：自动化工作流
- 文档问答：PDF/DOCX/图片支持
- 项目级隔离：独立配置与上下文
- 200+ 模型支持：OpenAI、Claude、Gemini、本地模型等

## 阅读指南

### 章节结构

每章遵循统一的结构模式，以便高效学习：

1. **问题** —— 说明该章节解决的实际工程问题
2. **模式** —— 讲解通用的设计模式或架构范式
3. **实现** —— 提供 BaoClaw 的 Rust 代码示例
4. **思考** —— 讨论替代方案与权衡决策
5. **总结** —— 要点总结与延伸阅读链接

## HTML 演示文稿使用指南

本书采用 HTML 幻灯片（Presentation/Slide）风格呈现，提供沉浸式的演示体验。

### 快速开始

#### 在线阅读

直接在浏览器中打开 `index.html` 文件即可开始阅读：

```bash
# 进入 book2 目录
cd book2

# 使用浏览器打开
open index.html        # macOS
xdg-open index.html    # Linux
start index.html       # Windows
```

#### 本地服务器

推荐使用本地服务器以获得完整功能（如 PWA 离线支持）：

```bash
# 使用 Python
python -m http.server 8080

# 使用 Node.js
npx serve .

# 使用 PHP
php -S localhost:8080
```

然后访问 http://localhost:8080

### 构建说明

#### 安装依赖

```bash
cd book2
npm install
```

#### 开发模式

```bash
# 启动开发服务器（热重载）
npm run dev
```

#### 构建生产版本

```bash
# 构建到 dist/ 目录
npm run build

# 预览构建结果
npm run preview
```

#### 运行测试

```bash
# 运行所有测试
npm test

# 运行测试并监听变化
npm run test:watch
```

### 导航操作

#### 键盘快捷键

| 操作 | 快捷键 | 说明 |
|------|--------|------|
| 下一页 | `→` `空格` `Enter` | 前进到下一张幻灯片 |
| 上一页 | `←` | 返回上一张幻灯片 |
| 首页 | `Home` | 跳转到第一张幻灯片 |
| 末页 | `End` | 跳转到最后一张幻灯片 |
| 全屏模式 | `f` `F` | 切换全屏显示 |
| 概览模式 | `o` `O` | 显示所有幻灯片缩略图 |
| 侧边栏 | `s` `S` | 切换章节目录侧边栏 |

#### 触摸手势（移动设备）

| 操作 | 手势 |
|------|------|
| 下一页 | 向左滑动（> 50px） |
| 上一页 | 向右滑动（> 50px） |

#### 鼠标操作

- 点击幻灯片右侧区域：下一页
- 点击幻灯片左侧区域：上一页
- 点击导航按钮：页面跳转

### 主题切换

支持深色/浅色两种主题：

- **自动检测**：首次访问时根据系统偏好自动选择
- **手动切换**：点击右上角主题按钮 🌙/☀️
- **持久保存**：主题偏好自动保存到浏览器本地存储

### 离线阅读（PWA）

本书支持 PWA（Progressive Web App）离线阅读：

1. **首次访问**：所有资源自动缓存
2. **离线使用**：断网后仍可正常阅读
3. **自动更新**：有新版本时提示刷新

#### 安装到桌面

在支持的浏览器中，可以将其安装为桌面应用：

- Chrome：地址栏右侧 → "安装"
- Safari：分享按钮 → "添加到主屏幕"

### 进度保存

阅读进度自动保存在浏览器本地存储：

- 当前阅读位置
- 已读幻灯片列表
- 最后访问时间

下次打开时自动恢复到上次阅读位置。

### URL 分享

每张幻灯片都有独立的 URL，便于分享：

```
https://your-domain.com/#/01-fundamentals/03
```

- `01-fundamentals`：章节 ID
- `03`：幻灯片序号

直接访问此 URL 即可跳转到指定幻灯片。

### 打印输出

支持打印为 PDF：

1. 使用浏览器打印功能（`Ctrl/Cmd + P`）
2. 选择"保存为 PDF"
3. 自动应用打印样式（隐藏导航控件、优化排版）

打印样式特性：
- A4 横向布局
- 每页一张幻灯片
- 隐藏 UI 控件
- 优化的字体大小

### 目录导航

左侧侧边栏提供完整的章节目录：

- **展开/折叠**：点击章节标题
- **快速跳转**：点击任意章节或幻灯片
- **当前高亮**：自动高亮当前阅读位置
- **隐藏侧边栏**：点击折叠按钮或按 `s` 键

### 代码块功能

所有代码示例来自 BaoClaw 真实源码：

```rust path="baoclaw-core/src/engine/query.rs" lines="45-78"
// Rust 代码示例
async fn execute_query(&self, input: QueryInput) -> Result<QueryOutput> {
    // ...
}
```

代码块头部标注：
- **path**：源文件相对路径
- **lines**：代码行号范围

点击代码块头部的路径可跳转到 GitHub 查看完整源码。

### 响应式设计

适配不同设备：

| 设备 | 特性 |
|------|------|
| 桌面端 | 完整功能、大字体、侧边栏 |
| 平板端 | 触摸导航、折叠侧边栏 |
| 手机端 | 全屏幻灯片、手势导航、隐藏非必要 UI |

## 章节目录

### 基础部分

**[第 1 章：Agent 基础](chapters/01-fundamentals/)**

- Agent 本质：从 LLM 到 Agent
- ReAct 循环：推理-行动-观察
- Harness 架构概览

### 核心实现

**[第 2 章：工具与扩展](chapters/02-core-implementation/)**

- 工具系统设计：Tool Trait 与执行器
- MCP 协议：让 Agent 获得无限能力
- Skills：可插拔的 Agent 行为

### 记忆与上下文

**[第 3 章：记忆系统](chapters/03-memory-context/)**

- 上下文管理：系统提示词的构建
- 短期记忆：对话历史与 Compact
- 长期记忆：跨会话的知识持久化
- 会话设计：共享、恢复与多客户端

### IPC 与多客户端

**[第 4 章：通信架构](chapters/04-ipc-multiclient/)**

- 守护进程架构：Daemon 模式
- IPC 协议：JSON-RPC over UDS
- 多客户端接入：终端、Telegram、更多
- 共享会话：SharedQueryEngine 的设计

### 生产实践

**[第 5 章：工程实践](chapters/05-production/)**

- 错误处理与恢复：从 Fallback 到自动 Compact
- 流式输出：SSE 事件与广播
- 权限控制：PermissionGate 模式
- 成本追踪：Token 计量与预算

### 高级模式

**[第 6 章：高级特性](chapters/06-advanced-patterns/)**

- Computer Use：桌面控制 Agent
- Agentic Coding：代码生成与编辑
- 多模型支持：Fallback 与模型切换

## 代码示例说明

本书所有代码示例均来自 BaoClaw 真实源码，格式如下：

```rust path="baoclaw-core/src/engine/query.rs" lines="45-78"
// Rust 代码示例
async fn execute_query(&self, input: QueryInput) -> Result<QueryOutput> {
    // ...
}
```

代码块头部标注了源文件路径和行号，方便读者在 GitHub 上查看完整实现。

## 技术支持

- **GitHub Issues**: [github.com/baohx/BaoClaw/issues](https://github.com/baohx/BaoClaw/issues)
- **源码仓库**: [github.com/baohx/BaoClaw](https://github.com/baohx/BaoClaw)

## 项目结构

```
book2/
├── README.md                    # 本书主入口（你正在阅读的文件）
├── book.config.ts               # 书籍配置文件
├── package.json                 # Node.js 项目配置
├── tsconfig.json                # TypeScript 配置
│
├── src/                         # 源代码
│   ├── parser/                  # Markdown 解析器
│   │   ├── markdown.ts          # 核心解析模块
│   │   ├── code-extractor.ts    # 代码块提取器
│   │   └── index.ts
│   │
│   ├── validator/               # 章节验证器
│   │   ├── section-validator.ts # 结构验证
│   │   └── index.ts
│   │
│   ├── generator/               # 幻灯片生成器
│   │   ├── slide.ts             # 幻灯片生成
│   │   ├── toc.ts               # 目录构建
│   │   └── index.ts
│   │
│   ├── renderer/                # 前端渲染
│   │   ├── slide-renderer.ts    # 幻灯片渲染器
│   │   ├── theme.ts             # 主题管理器
│   │   ├── syntax.ts            # 语法高亮器
│   │   └── index.ts
│   │
│   ├── navigation/              # 导航系统
│   │   ├── keyboard.ts          # 键盘导航
│   │   ├── touch.ts             # 触摸导航
│   │   ├── router.ts            # URL 路由
│   │   └── index.ts
│   │
│   ├── components/              # UI 组件
│   │   ├── sidebar.ts           # 侧边栏
│   │   ├── progress.ts          # 进度追踪
│   │   ├── controls.ts          # 控制按钮
│   │   └── index.ts
│   │
│   └── types/                   # TypeScript 类型定义
│       └── index.ts
│
├── chapters/                    # 章节内容
│   ├── 01-fundamentals/         # 第 1 章：Agent 基础
│   │   └── README.md
│   ├── 02-core-implementation/  # 第 2 章：核心实现
│   ├── 03-memory-context/       # 第 3 章：记忆系统
│   ├── 04-ipc-multiclient/      # 第 4 章：通信架构
│   ├── 05-production/           # 第 5 章：工程实践
│   └── 06-advanced-patterns/    # 第 6 章：高级特性
│
├── styles/                      # CSS 样式
│   ├── base.css                 # 基础样式和 CSS 变量
│   ├── slide.css                # 幻灯片样式
│   ├── code.css                 # 代码高亮样式
│   └── print.css                # 打印样式
│
├── scripts/                     # 构建脚本
│   └── build.ts                 # 主构建脚本
│
├── assets/                      # 静态资源
│   ├── images/                  # 图片
│   ├── diagrams/                # 架构图
│   └── fonts/                   # 字体
│
└── dist/                        # 构建输出（npm run build 后生成）
    ├── index.html
    ├── bundle.js
    ├── styles.css
    └── assets/
```

## 开发指南

### 环境要求

- Node.js >= 16.0.0
- npm >= 7.0.0

### 技术栈

| 类别 | 技术 |
|------|------|
| 语言 | TypeScript |
| 构建 | Node.js + ts-node |
| 测试 | Vitest |
| 语法高亮 | highlight.js |
| Markdown 解析 | marked |

### 添加新章节

1. 在 `chapters/` 下创建新目录：

```bash
mkdir chapters/07-new-chapter
```

2. 创建 `README.md`，使用章节模板：

```bash
cp CHAPTER_TEMPLATE.md chapters/07-new-chapter/README.md
```

3. 编辑内容，遵循"问题-模式-实现-思考"结构

4. 在 `book.config.ts` 中注册章节

5. 重新构建：

```bash
npm run build
```

### 自定义样式

修改 `styles/` 目录下的 CSS 文件：

- **base.css**：CSS 变量、字体、颜色主题
- **slide.css**：幻灯片布局和动画
- **code.css**：代码块语法高亮
- **print.css**：打印输出样式

### 验证章节

运行验证器检查章节结构：

```bash
npm run validate
```

验证规则：
- 必需部分是否存在（问题、模式、实现、思考）
- 代码块源文件路径格式
- 外部链接有效性

## License

本书内容采用 CC BY-NC-SA 4.0 协议。

BaoClaw 源代码采用 MIT 协议：[github.com/baohx/BaoClaw](https://github.com/baohx/BaoClaw)
