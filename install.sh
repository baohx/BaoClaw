#!/bin/bash
# BaoClaw Installer — installs baoclaw command globally
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="${BAOCLAW_HOME:-$HOME/.baoclaw}"
BIN_DIR="${BAOCLAW_BIN_DIR:-$HOME/.local/bin}"

echo "╔═══════════════════════════════════════╗"
echo "║       BaoClaw Installer v2.1.0        ║"
echo "╚═══════════════════════════════════════╝"
echo ""

# 1. Build Rust core
echo "🔨 Building Rust core (release)..."
cd "$SCRIPT_DIR/baoclaw-core"
cargo build --release 2>&1 | tail -3
cd "$SCRIPT_DIR"
echo "✓ Rust core built"

# 2. Install all TS gateway dependencies
for dir in ts-ipc baoclaw-telegram baoclaw-web baoclaw-feishu baoclaw-whatsapp; do
  if [ -d "$SCRIPT_DIR/$dir" ]; then
    echo "📦 Installing $dir dependencies..."
    cd "$SCRIPT_DIR/$dir"
    npm install --silent 2>&1
    cd "$SCRIPT_DIR"
    echo "✓ $dir ready"
  fi
done

# 3. Create install dirs
mkdir -p "$INSTALL_DIR/bin" "$BIN_DIR"

# 4. Copy Rust binary
cp "$SCRIPT_DIR/baoclaw-core/target/release/baoclaw-core" "$INSTALL_DIR/bin/baoclaw-core"
echo "✓ Rust binary → $INSTALL_DIR/bin/"

# 5. Copy each gateway source
copy_gateway() {
  local name="$1"
  local src="$SCRIPT_DIR/$name"
  local dst="$INSTALL_DIR/$name"
  [ ! -d "$src" ] && return 0
  mkdir -p "$dst/src" "$dst/public" "$dst/tui" 2>/dev/null
  # Kök seviye TS kaynakları (ts-ipc cli.ts / client.ts / vb. kökte tutar)
  for f in "$src"/*.ts "$src"/*.tsx; do
    [ -f "$f" ] && cp "$f" "$dst/"
  done
  # 复制 TS/TSX 源码
  for f in "$src"/src/*.ts "$src"/src/*.tsx; do
    [ -f "$f" ] && cp "$f" "$dst/src/"
  done
  # TUI 子目录（仅 ts-ipc）
  if [ "$name" = "ts-ipc" ] && [ -d "$src/tui" ]; then
    mkdir -p "$dst/tui/components"
    for f in "$src"/tui/*.ts "$src"/tui/*.tsx; do
      [ -f "$f" ] && cp "$f" "$dst/tui/"
    done
    for f in "$src"/tui/components/*.tsx; do
      [ -f "$f" ] && cp "$f" "$dst/tui/components/"
    done
  fi
  # public 静态资源（web）
  [ -d "$src/public" ] && cp -r "$src/public/." "$dst/public/" 2>/dev/null
  # 元信息
  cp "$src/package.json" "$dst/" 2>/dev/null
  cp "$src/package-lock.json" "$dst/" 2>/dev/null
  cp "$src/tsconfig.json" "$dst/" 2>/dev/null
  cd "$dst" && npm install --silent 2>&1 && cd "$SCRIPT_DIR"
  echo "✓ $name → $dst"
}
copy_gateway ts-ipc
copy_gateway baoclaw-telegram
copy_gateway baoclaw-web
copy_gateway baoclaw-feishu
copy_gateway baoclaw-whatsapp

# 6. Launcher 函数（统一生成非 CLI 客户端）
make_launcher() {
  local name="$1" target_subpath="$2" help="$3"
  cat > "$BIN_DIR/$name" << EOF
#!/bin/bash
# BaoClaw $name — $help
BAOCLAW_HOME="\${BAOCLAW_HOME:-\$HOME/.baoclaw}"
export BAOCLAW_CORE_BIN="\$BAOCLAW_HOME/bin/baoclaw-core"
exec npx --prefix "\$BAOCLAW_HOME/$(dirname "$target_subpath")" tsx "\$BAOCLAW_HOME/$target_subpath" "\$@"
EOF
  chmod +x "$BIN_DIR/$name"
  echo "✓ $name → $BIN_DIR/$name"
}

# CLI launcher（特殊：带 --help / --version）
cat > "$BIN_DIR/baoclaw" << 'LAUNCHER'
#!/bin/bash
BAOCLAW_HOME="${BAOCLAW_HOME:-$HOME/.baoclaw}"

if [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
  echo "BaoClaw v2.1.0 — AI coding assistant"
  echo ""
  echo "Usage: baoclaw [options]"
  echo ""
  echo "Options:"
  echo "  --sandbox <mode>  docker|bwrap|none"
  echo "  --think [budget]  Extended thinking"
  echo "  --vim             Vim mode"
  echo "  --debug           Debug mode"
  echo "  --version         Show version"
  echo "  --help            This help"
  echo ""
  echo "Config:  ~/.baoclaw/config.json (model_profiles, api keys)"
  echo "Clients: baoclaw, baoclaw-tui, baoclaw-web,"
  echo "          baoclaw-telegram, baoclaw-feishu, baoclaw-whatsapp"
  echo "Docs:    ~/.baoclaw/docs/USAGE.md"
  exit 0
fi

if [ "$1" = "--version" ] || [ "$1" = "-v" ]; then
  echo "baoclaw 2.1.0"
  exit 0
fi

export BAOCLAW_CORE_BIN="$BAOCLAW_HOME/bin/baoclaw-core"
exec npx --prefix "$BAOCLAW_HOME/ts-ipc" tsx "$BAOCLAW_HOME/ts-ipc/cli.ts" "$@"
LAUNCHER
chmod +x "$BIN_DIR/baoclaw"
echo "✓ baoclaw → $BIN_DIR/baoclaw"

# 其他客户端启动器
make_launcher "baoclaw-tui"       "ts-ipc/tui/index.tsx"           "Rich terminal UI (ink)"
make_launcher "baoclaw-web"       "baoclaw-web/src/server.ts"      "Web browser chat"
make_launcher "baoclaw-telegram"  "baoclaw-telegram/src/gateway.ts" "Telegram bot gateway"
make_launcher "baoclaw-feishu"    "baoclaw-feishu/src/gateway.ts"  "Feishu bot gateway"
make_launcher "baoclaw-whatsapp"  "baoclaw-whatsapp/src/session.ts" "WhatsApp gateway"

# 7. 创建 MCP 服务器启动脚本
mkdir -p "$INSTALL_DIR/bin"
if [ -f "$SCRIPT_DIR/scripts/mcp-servers.sh" ]; then
    cp "$SCRIPT_DIR/scripts/mcp-servers.sh" "$INSTALL_DIR/bin/mcp-servers"
    chmod +x "$INSTALL_DIR/bin/mcp-servers"
    echo "✓ mcp-servers → $INSTALL_DIR/bin/mcp-servers"
    echo "  Usage: $INSTALL_DIR/bin/mcp-servers {start|stop|restart|status|debug}"
fi

# 8. 复制文档到安装目录
mkdir -p "$INSTALL_DIR/docs"
[ -f "$SCRIPT_DIR/docs/USAGE.md" ] && cp "$SCRIPT_DIR/docs/USAGE.md" "$INSTALL_DIR/docs/" && echo "✓ docs/USAGE.md → $INSTALL_DIR/docs/"
[ -f "$SCRIPT_DIR/docs/DAEMON_MIGRATION.md" ] && cp "$SCRIPT_DIR/docs/DAEMON_MIGRATION.md" "$INSTALL_DIR/docs/" && echo "✓ docs/DAEMON_MIGRATION.md → $INSTALL_DIR/docs/"

# 9. 更新 systemd service（如果存在）
SYSTEMD_DIR="$HOME/.config/systemd/user"
if [ -d "$SYSTEMD_DIR" ] && [ -f "$SYSTEMD_DIR/baoclaw.service" ]; then
    # 检查是否需要添加 MCP 启动
    if ! grep -q "ExecStartPre.*mcp-servers" "$SYSTEMD_DIR/baoclaw.service" 2>/dev/null; then
        echo ""
        echo "⚠️  Updating systemd service to start MCP servers..."
        # 添加 ExecStartPre 启动 MCP 服务器
        BAOCLAW_BIN="$HOME/.baoclaw/bin"
        sed -i "s|ExecStart=${BAOCLAW_BIN}/baoclaw-core --daemon|ExecStartPre=${BAOCLAW_BIN}/mcp-servers start\nExecStart=${BAOCLAW_BIN}/baoclaw-core --daemon|" "$SYSTEMD_DIR/baoclaw.service"
        echo "✓ systemd service updated"
        echo "  Run: systemctl --user daemon-reload && systemctl --user restart baoclaw"
    fi
fi

echo ""
echo "═══════════════════════════════════════"
echo "  Installation complete!"
echo ""
echo "  Quick start:"
echo "    baoclaw              # terminal chat (auto-starts daemon)"
echo "    baoclaw-tui          # rich terminal UI"
echo "    baoclaw-web          # browser chat (http://localhost:8080)"
echo ""
echo "  Config: ~/.baoclaw/config.json"
echo "    { \"model_profiles\": { \"glm52\": { \"model\": \"glm-5.2\","
echo "        \"api_type\": \"anthropic\", \"api_key\": \"...\","
echo "        \"base_url\": \"...\", \"context_window\": 1000000 } },"
echo "      \"primary_profile\": \"glm52\" }"
echo ""
echo "  Optional: register daemon as system service"
echo "    Linux:   cp deploy/systemd/baoclaw.service ~/.config/systemd/user/"
echo "             systemctl --user enable --now baoclaw"
echo "    macOS:   cp deploy/launchd/com.baoclaw.daemon.plist ~/Library/LaunchAgents/"
echo "             launchctl load ~/Library/LaunchAgents/com.baoclaw.daemon.plist"
echo "    Windows: PowerShell -File deploy/windows/install.ps1"
echo ""
echo "  Docs: ~/.baoclaw/docs/USAGE.md"
echo "═══════════════════════════════════════"
