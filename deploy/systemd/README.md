# BaoClaw systemd Service (Linux user-level)

## 安装步骤

1. 编译 daemon：

   ```bash
   cd ~/BaoClaw
   cargo build --release --bin baoclaw-core
   ```

2. 复制 service 文件：

   ```bash
   mkdir -p ~/.config/systemd/user/
   cp deploy/systemd/baoclaw.service ~/.config/systemd/user/
   ```

3. 如需修改 ExecStart 路径，编辑 service 文件：

   ```bash
   nano ~/.config/systemd/user/baoclaw.service
   # 把 ExecStart 改为实际路径，如：
   # ExecStart=%h/BaoClaw/target/release/baoclaw-core --daemon
   ```

4. 重新加载 systemd 并启动：

   ```bash
   systemctl --user daemon-reload
   systemctl --user enable baoclaw        # 开机自启
   systemctl --user start baoclaw         # 立即启动
   systemctl --user status baoclaw        # 查看状态
   journalctl --user -u baoclaw -f        # 查看日志
   ```

5. 验证 daemon 已启动：
   ```bash
   ls -la $XDG_RUNTIME_DIR/baoclaw.sock
   # 应看到 /run/user/<UID>/baoclaw.sock
   ```

## 使用

daemon 常驻后，任何终端打开都能立即连接：

```bash
baoclaw                  # CLI 自动连 daemon
baoclaw-cli              # 独立 CLI 也连同一 daemon
```

## 卸载

```bash
systemctl --user stop baoclaw
systemctl --user disable baoclaw
rm ~/.config/systemd/user/baoclaw.service
systemctl --user daemon-reload
```
