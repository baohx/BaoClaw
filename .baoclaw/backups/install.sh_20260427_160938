#!/bin/bash
# BaoClaw Installer — installs baoclaw command globally
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="${BAOCLAW_HOME:-$HOME/.baoclaw}"
BIN_DIR="${BAOCLAW_BIN_DIR:-$HOME/.local/bin}"

echo "╔═══════════════════════════════════════╗"
echo "║       BaoClaw Installer v0.11.0        ║"
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

# 5c. Copy Web gateway files
echo "📦 Installing Web gateway..."
mkdir -p "$INSTALL_DIR/baoclaw-web/src"
mkdir -p "$INSTALL_DIR/baoclaw-web/public"
for f in "$SCRIPT_DIR"/baoclaw-web/src/*.ts; do
  [ -f "$f" ] && cp "$f" "$INSTALL_DIR/baoclaw-web/src/"
done
for f in "$SCRIPT_DIR"/baoclaw-web/public/*; do
  [ -f "$f" ] && cp "$f" "$INSTALL_DIR/baoclaw-web/public/"
done
cp "$SCRIPT_DIR/baoclaw-web/package.json" "$INSTALL_DIR/baoclaw-web/"
cp "$SCRIPT_DIR/baoclaw-web/package-lock.json" "$INSTALL_DIR/baoclaw-web/" 2>/dev/null || true
cp "$SCRIPT_DIR/baoclaw-web/tsconfig.json" "$INSTALL_DIR/baoclaw-web/"

# Install Web gateway deps in install dir
cd "$INSTALL_DIR/baoclaw-web"
npm install --silent 2>&1
cd "$SCRIPT_DIR"
echo "✓ Web gateway installed to $INSTALL_DIR/baoclaw-web/"

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
  echo "BaoClaw v0.11.0 — AI coding assistant"
  echo ""
  echo "Usage: baoclaw"
  echo ""
  echo "Environment variables:"
  echo "  Anthropic mode: ANTHROPIC_API_KEY, ANTHROPIC_BASE_URL"
  echo "  OpenAI mode:    OPENAI_API_KEY, OPENAI_BASE_URL"
  echo "  Set api_type in ~/.baoclaw/config.json"
  echo "  BAOCLAW_HOME    Install directory (default: ~/.baoclaw)"
  exit 0
fi

if [ "$1" = "--version" ] || [ "$1" = "-v" ]; then
  echo "baoclaw 0.13.4"
  exit 0
fi

# Check API key based on config api_type
BAOCLAW_CONFIG="$BAOCLAW_HOME/config.json"
API_TYPE="anthropic"
if [ -f "$BAOCLAW_CONFIG" ]; then
  API_TYPE=$(python3 -c "import json;print(json.load(open('$BAOCLAW_CONFIG')).get('api_type','anthropic'))" 2>/dev/null || echo "anthropic")
fi

if [ "$API_TYPE" = "openai" ]; then
  if [ -z "$OPENAI_API_KEY" ]; then
    echo "Error: OPENAI_API_KEY is not set (api_type=openai in config.json)"
    echo "  export OPENAI_API_KEY=sk-..."
    exit 1
  fi
else
  if [ -z "$ANTHROPIC_API_KEY" ]; then
    echo "Error: ANTHROPIC_API_KEY is not set."
    echo "  export ANTHROPIC_API_KEY=sk-ant-..."
    exit 1
  fi
fi

export BAOCLAW_CORE_BIN="$BAOCLAW_HOME/bin/baoclaw-core"
exec npx --prefix "$BAOCLAW_HOME/ts-ipc" tsx "$BAOCLAW_HOME/ts-ipc/cli.ts"
LAUNCHER

chmod +x "$BIN_DIR/baoclaw"
echo "✓ Launcher installed to $BIN_DIR/baoclaw"

# 6b. Create the web launcher script
cat > "$BIN_DIR/baoclaw-web" << 'WEBLAUNCHER'
#!/bin/bash
# BaoClaw Web — browser-based chat interface
# Usage: baoclaw-web [--port 8080]
BAOCLAW_HOME="${BAOCLAW_HOME:-$HOME/.baoclaw}"
exec npx --prefix "$BAOCLAW_HOME/baoclaw-web" tsx "$BAOCLAW_HOME/baoclaw-web/src/server.ts" "$@"
WEBLAUNCHER

chmod +x "$BIN_DIR/baoclaw-web"
echo "✓ Web launcher installed to $BIN_DIR/baoclaw-web"

echo ""
echo "═══════════════════════════════════════"
echo "  Installation complete!"
echo ""
echo "  Usage:"
echo "    export ANTHROPIC_API_KEY=sk-ant-..."
echo "    export ANTHROPIC_BASE_URL=https://your-proxy.com  # optional"
echo "    cd /path/to/your/project"
echo "    baoclaw              # terminal chat"
echo "    baoclaw-web          # browser chat (http://localhost:8080)"
echo "    baoclaw-web --port 9090"
echo ""
echo "  Add to ~/.bashrc for convenience:"
echo "    export ANTHROPIC_API_KEY=sk-ant-..."
echo "═══════════════════════════════════════"
