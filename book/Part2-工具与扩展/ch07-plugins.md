# 第 7 章：Plugins —— 打包分发的能力套件

## 7.1 问题：能力的组合与分发

一个完整的 Agent 能力通常需要多个组件配合：
- 一个 MCP 服务器提供底层工具
- 一个 Skill 提供使用指令
- 可能还需要配置文件

如何把这些打包成一个可分发的单元？

## 7.2 模式：Plugin = Skill + MCP + Tools 的容器

BaoClaw 的 Plugin 是一个目录，可以包含：

```
~/.baoclaw/plugins/computer-control/
├── plugin.json          # 清单文件
├── skills/              # Skill 文件
│   └── desktop-launch/
│       └── SKILL.md
├── mcp.json             # MCP 服务器配置
└── tools/               # 自定义工具（未来）
```

### plugin.json 清单

```json
{
    "name": "computer-control",
    "version": "1.0.0",
    "description": "桌面控制能力套件"
}
```

## 7.3 BaoClaw 的实现

### 发现（`claude-core/src/discovery/plugins.rs`）

```rust
pub async fn discover_plugins(cwd: &Path) -> Vec<PluginInfo> {
    // 扫描 ~/.baoclaw/plugins/ 和 <cwd>/.baoclaw/plugins/
    // 检测子目录中的 plugin.json、skills/、mcp.json、tools/
}
```

### 加载

Plugin 的加载分散在各个发现模块中：

- **Skills 加载**：`discover_skills` 会扫描 `plugins/*/skills/` 目录
- **MCP 加载**：`discover_mcp_servers` 会扫描 `plugins/*/mcp.json`
- **Tools 加载**：暂未实现（需要动态加载机制）

来源标记为 `plugin:<name>`，方便用户区分。

## 7.4 小结

Plugin 是能力的打包单元。通过标准的目录结构，一个 Plugin 可以同时提供 Skill 指令和 MCP 工具，实现"安装即可用"的体验。
