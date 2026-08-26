# BaoClaw 使用说明

BaoClaw 是一个 daemon 架构的 AI 编程助手。一个常驻 daemon 进程服务所有终端（CLI / TUI / Web / Telegram / 飞书 / WhatsApp），共享配置、session 池和记忆。

---

## 一、安装

### Linux / macOS

```bash
git clone https://github.com/baohx/BaoClaw.git
cd BaoClaw
./install.sh
```

- 安装目录：`~/.baoclaw/`
- 启动器目录：`~/.local/bin/`（请确保在 `$PATH` 中）
- 自动构建 Rust core + 安装所有 TS gateway 依赖
- 自动生成 6 个启动器：`baoclaw`、`baoclaw-tui`、`baoclaw-web`、`baoclaw-telegram`、`baoclaw-feishu`、`baoclaw-whatsapp`

### Windows

```powershell
git clone https://github.com/baohx/BaoClaw.git
cd BaoClaw
cd baoclaw-core
cargo build --release
cd ..\deploy\windows
PowerShell -ExecutionPolicy Bypass -File install.ps1
```

---

## 二、配置模型（~/.baoclaw/config.json）

编辑 `~/.baoclaw/config.json`，使用 `model_profiles` 表（支持主/退坡模型混搭 `api_type`）：

```json
{
  "model_profiles": {
    "glm52": {
      "model": "glm-5.2",
      "api_type": "anthropic",
      "api_key": "your-key-here",
      "base_url": "https://open.bigmodel.cn/api/anthropic",
      "context_window": 1000000,
      "auto_compact_threshold_ratio": 0.85
    },
    "ds": {
      "model": "deepseek-chat",
      "api_type": "openai",
      "api_key": "your-ds-key",
      "base_url": "https://api.deepseek.com",
      "context_window": 64000
    }
  },
  "primary_profile": "glm52",
  "fallback_profiles": ["ds"]
}
```

**旧格式**（`model` + `fallback_models` 字符串数组）仍然兼容，启动时自动迁移。

API key 优先级：`model_profiles.*.api_key` > 环境变量（`ANTHROPIC_API_KEY` / `OPENAI_API_KEY`）。

---

## 三、常驻进程（daemon）

### 方式 A：自动启动（默认，开箱即用）

**无需手动启动 daemon**。打开任何客户端时，如果 daemon 没在跑，会自动 fork 一个。

socket 路径：

- Linux: `$XDG_RUNTIME_DIR/baoclaw.sock`（通常是 `/run/user/<UID>/baoclaw.sock`）
- macOS: `/tmp/baoclaw-sockets/baoclaw.sock`
- Windows: `%TEMP%\baoclaw-sockets\baoclaw.sock`

### 方式 B：注册为系统服务（推荐生产环境）

更稳定、开机自启、崩溃自动重启。

#### Linux (systemd user service)

```bash
mkdir -p ~/.config/systemd/user/
cp deploy/systemd/baoclaw.service ~/.config/systemd/user/
# 如需修改 ExecStart 路径，编辑 service 文件
systemctl --user daemon-reload
systemctl --user enable --now baoclaw        # 开机自启 + 立即启动

# 管理命令
systemctl --user status baoclaw
systemctl --user restart baoclaw
systemctl --user stop baoclaw
journalctl --user -u baoclaw -f              # 查看日志
```

#### macOS (launchd)

```bash
cp deploy/launchd/com.baoclaw.daemon.plist ~/Library/LaunchAgents/
sed -i '' "s/YOUR_USERNAME/$(whoami)/g" ~/Library/LaunchAgents/com.baoclaw.daemon.plist
launchctl load ~/Library/LaunchAgents/com.baoclaw.daemon.plist
launchctl start com.baoclaw.daemon

# 管理
launchctl list | grep baoclaw
launchctl stop com.baoclaw.daemon
launchctl unload ~/Library/LaunchAgents/com.baoclaw.daemon.plist  # 卸载
```

#### Windows (Service)

```powershell
cd deploy\windows
PowerShell -ExecutionPolicy Bypass -File install.ps1

# 管理
Get-Service BaoClawDaemon
Start-Service BaoClawDaemon
Stop-Service BaoClawDaemon
Restart-Service BaoClawDaemon

# 卸载
PowerShell -ExecutionPolicy Bypass -File uninstall.ps1
```

### Daemon 如何优雅关闭

daemon 收到关闭信号（SIGTERM/SIGINT 或 Windows SCM Stop）时：

1. 触发 `persist_all()` — 把所有活跃 session 写入 `~/.baoclaw/sessions/`
2. 安全退出

**会话不会丢失**：daemon 重启后自动从磁盘恢复 session（消息历史 + 记忆摘要）。

---

## 四、各个渠道如何开启

> **所有渠道共享同一个 daemon**。无论从哪个渠道发消息，都走同一个 IPC，看到同一份 session。

### 1. CLI（终端聊天，最常用）

```bash
baoclaw                  # 默认连 daemon（没在跑会自动 fork）
baoclaw --sandbox docker # Docker 沙箱模式
baoclaw --think          # 开启扩展思考
baoclaw --vim            # Vim 模式
baoclaw --debug          # 调试模式
```

**退出**：输入 `/exit` 或按 `Ctrl+C`

### 2. TUI（React + ink 的富终端 UI）

```bash
baoclaw-tui              # 需要 daemon 已在跑（systemd 或先开一次 baoclaw）
```

**退出**：按 `q` 或 `Ctrl+C`

### 3. Web（浏览器聊天）

```bash
baoclaw-web              # 默认 http://localhost:8080
baoclaw-web --port 9090  # 自定义端口
```

打开浏览器访问 `http://localhost:8080`。**退出**：`Ctrl+C`

### 4. Telegram Bot

```bash
baoclaw-telegram         # 长驻进程，监听 Telegram updates
```

**前提**：`~/.baoclaw/config.json` 中 `telegram.token` 已配置。
**退出**：`Ctrl+C`

### 5. 飞书 Bot

```bash
baoclaw-feishu           # 长驻进程，监听飞书事件
```

**前提**：飞书应用凭证已配置。
**退出**：`Ctrl+C`

### 6. WhatsApp

```bash
baoclaw-whatsapp         # 长驻进程
```

**前提**：`~/.baoclaw/config.json` 中 `whatsapp.phoneNumber` 已配置。
**退出**：`Ctrl+C`

---

## 五、实用斜杠命令（CLI/TUI 通用）

```
/help        查看所有命令
/tokens      查看 token 用量（当前 / 累计 / 距离压缩）
/cost        查看花费估算
/memory      记忆系统说明（/memory list 查看条目）
/model       当前模型配置（key 自动打码）
/config      完整配置 JSON（key 自动打码）
/session     当前 session 信息
/clear       清空当前会话上下文
/exit        退出
```

---

## 六、验证 daemon 是否在跑

### Linux

```bash
ls -la $XDG_RUNTIME_DIR/baoclaw.sock
systemctl --user status baoclaw
```

### macOS

```bash
ls -la /tmp/baoclaw-sockets/baoclaw.sock
launchctl list | grep baoclaw
```

### Windows

```powershell
Get-Service BaoClawDaemon
ls $env:TEMP\baoclaw-sockets\baoclaw.sock
```

---

## 七、目录结构

```
~/.baoclaw/
├── bin/
│   └── baoclaw-core              # Rust daemon 二进制
├── ts-ipc/                       # CLI + TUI 源码
├── baoclaw-web/                  # Web gateway
├── baoclaw-telegram/             # Telegram gateway
├── baoclaw-feishu/               # 飞书 gateway
├── baoclaw-whatsapp/             # WhatsApp gateway
├── docs/                         # 文档（USAGE.md / DAEMON_MIGRATION.md）
├── config.json                   # 配置文件（model_profiles）
├── memories/                     # 长期记忆（JSONL）
└── sessions/                     # 会话持久化
    ├── registry.json             # session 索引
    ├── <session-id>.json         # 单个 session 状态
    └── archive/                  # >7 天不活跃的归档
```

---

## 八、故障排查

### daemon 启动失败

```bash
# 检查 socket 文件是否被占用（stale socket）
ls -la /run/user/$(id -u)/baoclaw.sock

# 如果是 stale socket（daemon 已死但文件残留），删除它
rm /run/user/$(id -u)/baoclaw.sock

# 重新启动
systemctl --user restart baoclaw      # Linux
launchctl start com.baoclaw.daemon    # macOS
Start-Service BaoClawDaemon           # Windows
```

### 客户端连不上 daemon

```bash
# 1. 确认 daemon 在运行
systemctl --user status baoclaw

# 2. 确认 socket 文件存在
ls -la /run/user/$(id -u)/baoclaw.sock

# 3. 查看日志
journalctl --user -u baoclaw -f      # Linux
tail -f /tmp/baoclaw-daemon.stderr.log  # macOS
Get-EventLog -LogName Application -Source BaoClawDaemon  # Windows
```

### session 丢失

```bash
# 检查持久化文件
ls ~/.baoclaw/sessions/

# 检查索引
cat ~/.baoclaw/sessions/registry.json | python3 -m json.tool

# daemon 启动时会自动 load_from_disk()，通常无需手动恢复
```

### API key 不生效

1. 检查 `~/.baoclaw/config.json` 的 `model_profiles.*.api_key`
2. 如果用环境变量，检查 `ANTHROPIC_API_KEY` / `OPENAI_API_KEY`
3. 优先级：`config.json` 的 `api_key` > 环境变量

---

## 九、卸载

### 仅卸载客户端（保留 daemon 和配置）

```bash
rm ~/.local/bin/baoclaw*
rm -rf ~/.baoclaw/ts-ipc ~/.baoclaw/baoclaw-*
```

### 完全卸载

```bash
# 1. 停止并卸载服务
systemctl --user stop baoclaw
systemctl --user disable baoclaw
rm ~/.config/systemd/user/baoclaw.service
systemctl --user daemon-reload

# 2. 删除安装目录和配置
rm -rf ~/.baoclaw/
rm ~/.local/bin/baoclaw*
```

---

## 十、更多文档

- [Daemon 架构迁移指南](DAEMON_MIGRATION.md) — 从旧版 PID socket 迁移到固定 socket + systemd
- [systemd 服务安装](../deploy/systemd/README.md)
- [launchd 服务安装](../deploy/launchd/README.md)
- [Windows Service 安装](../deploy/windows/README.md)

---

**版本**：v2.1.0  
**最后更新**：2026-06-19
