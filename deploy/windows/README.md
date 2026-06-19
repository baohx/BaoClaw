# BaoClaw Windows Service

BaoClaw 可以作为 Windows Service 运行，实现开机自启、后台常驻、统一管理。

## 前置条件

- Windows 10 / 11 或 Windows Server 2016+
- PowerShell 5.1+
- 已编译的 `baoclaw-core.exe`（见下方编译步骤）
- 管理员权限（安装/卸载服务需要）

## 编译

```powershell
cd baoclaw-core
cargo build --release
```

编译产物：`baoclaw-core\target\release\baoclaw-core.exe`

## 一键安装（推荐）

以**管理员身份**打开 PowerShell，运行：

```powershell
cd C:\path\to\BaoClaw
PowerShell -ExecutionPolicy Bypass -File deploy\windows\install.ps1
```

脚本会自动：
1. 定位 `baoclaw-core.exe`
2. 通过 `sc.exe create` 注册服务（start=auto，开机自启）
3. 设置服务描述
4. 启动服务
5. 验证服务状态

安装完成后，服务会**开机自动启动**。

## 手动安装（sc.exe）

如果不使用脚本，也可以手动操作：

```powershell
# 1. 注册服务（binPath 必须包含 --run-as-service 参数）
sc create BaoClawDaemon binPath= "\"C:\path\to\baoclaw-core.exe\" --run-as-service" DisplayName= "BaoClaw AI Coding Assistant" start= auto

# 2. 设置描述
sc description BaoClawDaemon "Long-running daemon for BaoClaw. Provides IPC, session management, and tool execution for all BaoClaw clients."

# 3. 启动
net start BaoClawDaemon
```

或者使用二进制自带的安装命令：

```powershell
.\baoclaw-core.exe --install-service
net start BaoClawDaemon
```

## 验证服务运行

```powershell
# 查看服务状态
Get-Service BaoClawDaemon

# 或用 sc.exe
sc query BaoClawDaemon

# 检查 socket 文件
Test-Path "$env:TEMP\baoclaw-sockets\baoclaw.sock"
```

服务运行时，所有 BaoClaw 客户端（CLI、Web、Telegram 等）会自动连接到同一 socket：
```
%TEMP%\baoclaw-sockets\baoclaw.sock
```

## 管理命令

```powershell
# 启动
Start-Service BaoClawDaemon
# 或
net start BaoClawDaemon

# 停止
Stop-Service BaoClawDaemon
# 或
net stop BaoClawDaemon

# 重启
Restart-Service BaoClawDaemon

# 查看状态
Get-Service BaoClawDaemon

# 查看详细配置
sc qc BaoClawDaemon
```

也可以通过 `services.msc`（在开始菜单搜索 "services"）图形化管理。

## 卸载

### 一键卸载

以**管理员身份**运行：

```powershell
PowerShell -ExecutionPolicy Bypass -File deploy\windows\uninstall.ps1
```

### 手动卸载

```powershell
# 1. 停止服务
net stop BaoClawDaemon

# 2. 删除服务
sc delete BaoClawDaemon
```

或使用二进制自带的卸载命令：

```powershell
.\baoclaw-core.exe --uninstall-service
```

## 故障排查

### 服务无法启动

1. **检查事件查看器**：在开始菜单搜索 "Event Viewer" → Windows Logs → Application，查找 BaoClawDaemon 相关错误。
2. **检查 sc.exe 查询**：
   ```powershell
   sc query BaoClawDaemon
   ```
   如果状态为 `STOPPED` 且 `WIN32_EXIT_CODE` 非零，说明启动失败。
3. **手动测试运行**：
   ```powershell
   .\baoclaw-core.exe --daemon
   ```
   直接前台运行看是否有错误输出。
4. **检查路径**：确保 `sc qc BaoClawDaemon` 显示的 binPath 正确，且包含 `--run-as-service` 参数。

### 客户端连不上 daemon

1. 确认服务正在运行：`Get-Service BaoClawDaemon`
2. 确认 socket 存在：`Test-Path "$env:TEMP\baoclaw-sockets\baoclaw.sock"`
3. 如果 socket 不存在，检查 `%TEMP%\baoclaw-sockets\` 目录是否可写。

### 服务卡在 START_PENDING

服务的 binPath 中 `--run-as-service` 模式会启动一个子进程（`--daemon`），如果子进程启动失败，服务会超时。检查事件查看器获取详细信息。

## Socket 路径说明

| 平台 | Socket 路径 |
|------|------------|
| Windows | `%TEMP%\baoclaw-sockets\baoclaw.sock` |
| Linux | `$XDG_RUNTIME_DIR/baoclaw.sock`（通常 `/run/user/<UID>/baoclaw.sock`） |
| macOS | `/tmp/baoclaw-sockets/baoclaw.sock` |

所有客户端（CLI、Web UI、Telegram Bot 等）都连接到同一 socket，确保会话共享。

## 开发模式（前台运行）

开发时可以不用服务模式，直接前台运行：

```powershell
.\baoclaw-core.exe --daemon
```

这与 Linux 的 `--daemon` 模式完全一致，只是没有 Windows Service 的 SCM 管理。

## 与其他客户端的兼容性

BaoClaw 采用**单 daemon + 多客户端**架构：

```
                    ┌──────────────┐
                    │  BaoClawDaemon │ (Windows Service / systemd / launchd)
                    │  (常驻进程)     │
                    └──────┬───────┘
                           │ IPC (Unix domain socket / Named pipe)
          ┌────────────────┼────────────────┐
          │                │                │
    ┌─────┴─────┐   ┌─────┴─────┐   ┌─────┴─────┐
    │  CLI      │   │  Web UI   │   │ Telegram  │
    │ (baoclaw) │   │ (浏览器)  │   │   Bot     │
    └───────────┘   └───────────┘   └───────────┘
```

所有客户端连接同一个 daemon，共享会话状态、内存和配置。

## 技术实现

- 使用 [`windows-service`](https://crates.io/crates/windows-service) crate 与 Windows SCM（Service Control Manager）交互
- 服务架构：**服务宿主进程**（管理 SCM 连接）+ **daemon 子进程**（`--daemon` 模式运行）
- 当 SCM 发出停止指令时，服务宿主设置 shutdown 标志，daemon 子进程检测后优雅退出（persist sessions）
- 条件编译：`#[cfg(target_os = "windows")]`，Linux/macOS 编译完全不受影响
