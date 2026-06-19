# BaoClaw launchd Service (macOS)

## 安装步骤

1. 编译 daemon：
   ```bash
   cd ~/BaoClaw
   cargo build --release --bin baoclaw-core
   ```

2. 复制 plist：
   ```bash
   cp deploy/launchd/com.baoclaw.daemon.plist ~/Library/LaunchAgents/
   ```

3. 修改 plist 中的 `YOUR_USERNAME`：
   ```bash
   sed -i '' "s/YOUR_USERNAME/$(whoami)/g" ~/Library/LaunchAgents/com.baoclaw.daemon.plist
   ```

4. 加载并启动：
   ```bash
   launchctl load ~/Library/LaunchAgents/com.baoclaw.daemon.plist
   launchctl start com.baoclaw.daemon
   ```

5. 验证：
   ```bash
   ls -la /tmp/baoclaw-sockets/baoclaw.sock
   tail -f /tmp/baoclaw-daemon.stderr.log
   ```

## 卸载

```bash
launchctl unload ~/Library/LaunchAgents/com.baoclaw.daemon.plist
rm ~/Library/LaunchAgents/com.baoclaw.daemon.plist
```
