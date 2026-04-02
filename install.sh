#!/bin/bash
# BaoClaw Installer — installs baoclaw command globally
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="${BAOCLAW_HOME:-$HOME/.baoclaw}"
BIN_DIR="${BAOCLAW_BIN_DIR:-$HOME/.local/bin}"

echo "╔═══════════════════════════════════════╗"
echo "║       BaoClaw Installer v0.1.0        ║"
echo "╚═══════════════════════════════════════╝"
echo ""

# 1. Build Rust core
echo "🔨 Building Rust core (release)..."
cd "$SCRIPT_DIR/claude-core"
cargo build --release 2>&1 | tail -3
cd "$SCRIPT_DIR"
echo "✓ Rust core built"

# 2. Install TS dependencies
echo "📦 Installing TypeScript dependencies..."
cd "$SCRIPT_DIR/ts-ipc"
npm install --silent 2>&1
cd "$SCRIPT_DIR"
echo "✓ Dependencies installed"

# 3. Create install directory
mkdir -p "$INSTALL_DIR/bin"
mkdir -p "$BIN_DIR"

# 4. Copy Rust binary
cp "$SCRIPT_DIR/claude-core/target/release/claude-core" "$INSTALL_DIR/bin/claude-core"
echo "✓ Rust binary installed to $INSTALL_DIR/bin/"

# 5. Copy TS-IPC files
mkdir -p "$INSTALL_DIR/ts-ipc"
cp "$SCRIPT_DIR/ts-ipc/cli.ts" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/client.ts" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/index.ts" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/streamHandler.ts" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/types.ts" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/rustCore.ts" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/useRustEngine.ts" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/package.json" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/package-lock.json" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/tsconfig.json" "$INSTALL_DIR/ts-ipc/"

# Install TS deps in install dir
cd "$INSTALL_DIR/ts-ipc"
npm install --silent 2>&1
cd "$SCRIPT_DIR"
echo "✓ TypeScript files installed to $INSTALL_DIR/ts-ipc/"

# 6. Create the launcher script
cat > "$BIN_DIR/baoclaw" << 'LAUNCHER'
#!/bin/bash
# BaoClaw — AI coding assistant
# Usage: baoclaw [options]
#   Options:
#     --help    Show this help
#     --version Show version

BAOCLAW_HOME="${BAOCLAW_HOME:-$HOME/.baoclaw}"

if [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
  echo "BaoClaw v0.1.0 — AI coding assistant"
  echo ""
  echo "Usage: baoclaw"
  echo ""
  echo "Environment variables:"
  echo "  ANTHROPIC_API_KEY      API key (required)"
  echo "  ANTHROPIC_BASE_URL     Custom API endpoint"
  echo "  BAOCLAW_HOME           Install directory (default: ~/.baoclaw)"
  exit 0
fi

if [ "$1" = "--version" ] || [ "$1" = "-v" ]; then
  echo "baoclaw 0.1.0"
  exit 0
fi

if [ -z "$ANTHROPIC_API_KEY" ]; then
  echo "Error: ANTHROPIC_API_KEY is not set."
  echo "  export ANTHROPIC_API_KEY=sk-ant-..."
  echo "  baoclaw"
  exit 1
fi

export CLAUDE_CORE_BIN="$BAOCLAW_HOME/bin/claude-core"
exec npx --prefix "$BAOCLAW_HOME/ts-ipc" tsx "$BAOCLAW_HOME/ts-ipc/cli.ts"
LAUNCHER

chmod +x "$BIN_DIR/baoclaw"
echo "✓ Launcher installed to $BIN_DIR/baoclaw"

echo ""
echo "═══════════════════════════════════════"
echo "  Installation complete!"
echo ""
echo "  Usage:"
echo "    export ANTHROPIC_API_KEY=sk-ant-..."
echo "    export ANTHROPIC_BASE_URL=https://your-proxy.com  # optional"
echo "    cd /path/to/your/project"
echo "    baoclaw"
echo ""
echo "  Add to ~/.bashrc for convenience:"
echo "    export ANTHROPIC_API_KEY=sk-ant-..."
echo "═══════════════════════════════════════"
