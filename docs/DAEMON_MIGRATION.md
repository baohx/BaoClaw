# Daemon 架构迁移指南

本文档介绍 BaoClaw daemon 架构的演进和如何从旧版迁移到新版。

## 架构演进

### Phase 0（原始）：PID-based socket
- socket 路径：`/tmp/baoclaw-sockets/baoclaw-<PID>.sock`
- 问题：每个 CLI 启动都 fork 新 daemon，同 cwd 也不能共享 session

### Phase 1（P1-2，2026-06-19）：cwd-hash socket
- socket 路径：`/tmp/baoclaw-sockets/baoclaw-cwd-<16hex>.sock`
- 改进：同一 cwd 的所有客户端共享 daemon
- 限制：仍依赖 cwd，跨目录要切

### Phase 2（P3-1c，2026-06-19）：固定 socket + 优雅关闭
- socket 路径：
  - Linux: `$XDG_RUNTIME_DIR/baoclaw.sock`（/run/user/UID/）
  - macOS: `/tmp/baoclaw-sockets/baoclaw.sock`
  - Windows: `%TEMP%/baoclaw-sockets/baoclaw.sock`
- 改进：机器级单 daemon，所有 session 共享
- 优雅关闭：SIGTERM/SIGINT 时 persist_all()

### Phase 3（P3-1a/b，2026-06-19）：systemd/launchd 服务化
- daemon 7×24 常驻
- 开机自启
- 崩溃自动重启

## 连接逻辑

客户端按以下顺序查找 daemon：

1. **固定 socket**（`fixed_socket_path()`）
   - 优先级最高，systemd/launchd 服务化后用这个
2. **cwd-hash socket**（`make_socket_path(cwd)`）
   - Fallback，兼容未服务化的环境

如果两者都不存在，CLI 会自己 fork 一个 daemon（旧行为，向后兼容）。

## 迁移步骤

### 从 Phase 0/1 迁移到 Phase 2（自动，无需操作）

升级到 `3252fb8` 或更新版本后，客户端连接逻辑自动变为：
- 先找固定 socket（新行为）
- 找不到再找 cwd-hash socket（旧行为）
- 都找不到则 fork 新 daemon

**无需修改任何配置**。

### 从 Phase 2 迁移到 Phase 3（手动，可选）

如果想用 systemd 服务化（Linux）：

1. 按 `deploy/systemd/README.md` 安装 service
2. 启动 service：`systemctl --user start baoclaw`
3. 验证：`ls $XDG_RUNTIME_DIR/baoclaw.sock`
4. 此后所有终端打开都立即连 daemon，无需 CLI 自己 fork

### 清理旧 socket 文件

升级后可能残留旧 socket 文件，可清理：
```bash
# Linux/macOS
rm -f /tmp/baoclaw-sockets/baoclaw-*.sock
rm -f /tmp/baoclaw-sockets/baoclaw-cwd-*.sock
rm -f /run/user/$(id -u)/baoclaw-cwd-*.sock

# 只保留固定 socket
ls /run/user/$(id -u)/baoclaw.sock          # Linux
ls /tmp/baoclaw-sockets/baoclaw.sock         # macOS
```

## Session 持久化

Phase 2 引入 session 持久化：
- 存储路径：`~/.baoclaw/sessions/<session-id>.json`
- 索引文件：`~/.baoclaw/sessions/registry.json`
- 归档目录：`~/.baoclaw/sessions/archive/`（>7 天不活跃自动归档）
- 触发时机：每轮对话结束 + daemon 收到 SIGTERM/SIGINT

daemon 崩溃或重启后，启动时会自动从磁盘恢复 session（消息历史 + 记忆摘要）。

## 故障排查

### daemon 启动失败

```bash
# 检查 socket 文件是否被占用
ls -la /run/user/$(id -u)/baoclaw.sock

# 如果是 stale socket（daemon 已死但文件残留），删除它
rm /run/user/$(id -u)/baoclaw.sock

# 重新启动 daemon
systemctl --user restart baoclaw
```

### 客户端连不上 daemon

```bash
# 1. 确认 daemon 在运行
systemctl --user status baoclaw

# 2. 确认 socket 文件存在
ls -la /run/user/$(id -u)/baoclaw.sock

# 3. 测试连接（如果有 socat）
socat - UNIX-CONNECT:/run/user/$(id -u)/baoclaw.sock

# 4. 查看日志
journalctl --user -u baoclaw -f
```

### session 丢失

```bash
# 检查持久化文件
ls ~/.baoclaw/sessions/

# 检查 registry
cat ~/.baoclaw/sessions/registry.json | jq .

# 手动恢复（通常自动恢复）
# daemon 启动时会自动 load_from_disk()
```

## 回滚

如果新版有问题，可以回滚到旧版：
```bash
git checkout 929e161    # Phase 0/1
cargo build --release --bin baoclaw-core
```

session 持久化文件（`~/.baoclaw/sessions/`）在新旧版本间兼容（都是 JSON），无需清理。
