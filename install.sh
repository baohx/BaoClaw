#!/bin/bash
# BaoClaw Installer — installs baoclaw command globally
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="${BAOCLAW_HOME:-$HOME/.baoclaw}"
BIN_DIR="${BAOCLAW_BIN_DIR:-$HOME/.local/bin}"

echo "╔═══════════════════════════════════════╗"
echo "║       BaoClaw Installer v0.6.0        ║"
echo "╚═══════════════════════════════════════╝"
echo ""

# 1. Build Rust core
echo "🔨 Building Rust core (release)..."
cd "$SCRIPT_DIR/baoclaw-core"
cargo build --release 2>&1 | tail -3
cd "$SCRIPT_DIR"
echo "✓ Rust core built"

# 2. Install TS dependencies
echo "📦 Installing TypeScript dependencies..."
cd "$SCRIPT_DIR/ts-ipc"
npm install --silent 2>&1
cd "$SCRIPT_DIR"
echo "✓ Dependencies installed"

# 2b. Install Telegram gateway dependencies
echo "📦 Installing Telegram gateway dependencies..."
cd "$SCRIPT_DIR/baoclaw-telegram"
npm install --silent 2>&1
cd "$SCRIPT_DIR"
echo "✓ Telegram gateway dependencies installed"

# 3. Create install directory
mkdir -p "$INSTALL_DIR/bin"
mkdir -p "$BIN_DIR"

# 4. Copy Rust binary
cp "$SCRIPT_DIR/baoclaw-core/target/release/baoclaw-core" "$INSTALL_DIR/bin/baoclaw-core"
echo "✓ Rust binary installed to $INSTALL_DIR/bin/"

# 5. Copy TS-IPC files
mkdir -p "$INSTALL_DIR/ts-ipc"
# Copy all TypeScript source files
for f in "$SCRIPT_DIR"/ts-ipc/*.ts; do
  [ -f "$f" ] && cp "$f" "$INSTALL_DIR/ts-ipc/"
done
cp "$SCRIPT_DIR/ts-ipc/package.json" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/package-lock.json" "$INSTALL_DIR/ts-ipc/"
cp "$SCRIPT_DIR/ts-ipc/tsconfig.json" "$INSTALL_DIR/ts-ipc/"

# Install TS deps in install dir
cd "$INSTALL_DIR/ts-ipc"
npm install --silent 2>&1
cd "$SCRIPT_DIR"
echo "✓ TypeScript files installed to $INSTALL_DIR/ts-ipc/"

# 5b. Copy Telegram gateway files
mkdir -p "$INSTALL_DIR/baoclaw-telegram/src"
for f in "$SCRIPT_DIR"/baoclaw-telegram/src/*.ts; do
  [ -f "$f" ] && cp "$f" "$INSTALL_DIR/baoclaw-telegram/src/"
done
cp "$SCRIPT_DIR/baoclaw-telegram/package.json" "$INSTALL_DIR/baoclaw-telegram/"
cp "$SCRIPT_DIR/baoclaw-telegram/package-lock.json" "$INSTALL_DIR/baoclaw-telegram/" 2>/dev/null || true
cp "$SCRIPT_DIR/baoclaw-telegram/tsconfig.json" "$INSTALL_DIR/baoclaw-telegram/"

# Install Telegram gateway deps in install dir
cd "$INSTALL_DIR/baoclaw-telegram"
npm install --silent 2>&1
cd "$SCRIPT_DIR"
echo "✓ Telegram gateway installed to $INSTALL_DIR/baoclaw-telegram/"

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
  echo "BaoClaw v0.6.0 — AI coding assistant"
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
  echo "baoclaw 0.6.0"
  exit 0
fi

if [ -z "$ANTHROPIC_API_KEY" ]; then
  echo "Error: ANTHROPIC_API_KEY is not set."
  echo "  export ANTHROPIC_API_KEY=sk-ant-..."
  echo "  baoclaw"
  exit 1
fi

export BAOCLAW_CORE_BIN="$BAOCLAW_HOME/bin/baoclaw-core"
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
