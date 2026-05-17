# 第 6 章：Skills —— 可插拔的 Agent 行为

## 6.1 问题：工具 vs 指令

工具（Tool）给 Agent 行动能力，但有些能力不是"调用一个函数"能解决的。比如：

- "用 xdotool 截图分析桌面图标然后点击" —— 这是一个多步骤的工作流
- "模拟一场辩论，正方反方交替发言" —— 这是一种对话模式

这些不是单个工具调用，而是**行为指令** —— 告诉 Agent "遇到这种场景时，按这个流程做"。

## 6.2 模式：Skill = 系统提示词注入

BaoClaw 的 Skill 本质上是一段 Markdown 文本，在 daemon 启动时被注入到系统提示词中。AI 在每次对话时都能看到这些指令。

Skill 文件格式（`~/.baoclaw/skills/desktop-launch-app/SKILL.md`）：

```markdown
---
name: desktop-launch-app
description: 在桌面上通过开始菜单启动应用程序
allowed-tools:
  - Bash
  - FileWrite
---

# 通过开始菜单启动桌面应用

通过视觉截图分析 + 鼠标点击的方式，在桌面环境中打开开始菜单并启动指定应用程序。

## 步骤
1. 使用 xdotool 点击左下角启动器
2. 等待菜单出现
3. 截图分析菜单内容
4. 找到目标应用图标并点击
```

YAML frontmatter 包含元数据（名称、描述、允许的工具），Markdown body 包含详细指令。

## 6.3 BaoClaw 的实现

### 发现（`claude-core/src/discovery/skills.rs`）

```rust
pub async fn discover_skills(cwd: &Path) -> Vec<SkillInfo> {
    // 扫描三个位置：
    // 1. 项目级: <cwd>/.baoclaw/skills/
    // 2. 用户级: ~/.baoclaw/skills/
    // 3. 插件级: ~/.baoclaw/plugins/*/skills/
}
```

支持两种文件格式：
- 目录格式：`skill-name/SKILL.md`
- 单文件格式：`skill-name.md`

### 加载与注入

```rust
pub async fn load_skills_for_prompt(cwd: &Path) -> Option<String> {
    let skills = discover_skills(cwd).await;
    if skills.is_empty() { return None; }
    
    let mut parts = vec!["# Loaded Skills\n\n...".to_string()];
    for skill in &skills {
        let content = fs::read_to_string(&skill.path).await?;
        parts.push(format!("## Skill: {} [source: {}]\n\n{}", 
            skill.name, skill.source, content));
    }
    Some(parts.join("\n"))
}
```

加载后的内容通过 `append_system_prompt` 注入到 `build_system_prompt` 中，AI 在每次 API 调用时都能看到。

## 6.4 Skill vs Tool 的选择

| 维度 | Skill | Tool |
|------|-------|------|
| 本质 | 文本指令 | 可执行函数 |
| 注入方式 | 系统提示词 | 工具列表 |
| 消耗 | 占用上下文窗口 | 不占用（直到被调用） |
| 适用场景 | 多步骤工作流、对话模式 | 单一原子操作 |
| 创建难度 | 写 Markdown | 写 Rust 代码 |

## 6.5 小结

Skill 是 Agent 的"说明书"，通过系统提示词注入让 AI 学会新的行为模式。它的优势是零代码 —— 任何人都可以用 Markdown 创建新的 Skill。
