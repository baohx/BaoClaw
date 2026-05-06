#!/bin/bash
# BaoClaw — Launch Script
# Usage: ANTHROPIC_API_KEY=sk-ant-... ./start.sh
#        OPENAI_API_KEY=sk-... ./start.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Read api_type from config to decide which key to check
read_api_type() {
  local config="$HOME/.baoclaw/config.json"
  if [ -f "$config" ]; then
    # Use sed to extract api_type value: "api_type": "openai"
    local val=$(sed -n 's/.*"api_type"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$config" | head -1)
    if [ -n "$val" ]; then
      echo "$val"
      return
    fi
  fi
  echo "anthropic"
}

API_TYPE=$(read_api_type)

# Check API key based on api_type
if [ "$API_TYPE" = "openai" ]; then
  if [ -z "$OPENAI_API_KEY" ]; then
    echo "╔══════════════════════════════════════════════╗"
    echo "║  OPENAI_API_KEY is not set.                  ║"
    echo "║  (api_type is 'openai' in config.json)       ║"
    echo "║                                              ║"
    echo "║  Usage:                                      ║"
    echo "║    export OPENAI_API_KEY=sk-...              ║"
    echo "║    ./start.sh                                ║"
    echo "║                                              ║"
    echo "║  For custom API endpoint:                    ║"
    echo "║    export OPENAI_BASE_URL=https://...        ║"
    echo "╚══════════════════════════════════════════════╝"
    exit 1
  fi
else
  if [ -z "$ANTHROPIC_API_KEY" ]; then
    echo "╔══════════════════════════════════════════════╗"
    echo "║  ANTHROPIC_API_KEY is not set.               ║"
    echo "║  (api_type is 'anthropic' in config.json)    ║"
    echo "║                                              ║"
    echo "║  Usage:                                      ║"
    echo "║    export ANTHROPIC_API_KEY=sk-ant-...       ║"
    echo "║    ./start.sh                                ║"
    echo "║                                              ║"
    echo "║  To use OpenAI instead:                      ║"
    echo "║    Set \"api_type\": \"openai\" in               ║"
    echo "║    ~/.baoclaw/config.json                    ║"
    echo "║    Then: export OPENAI_API_KEY=sk-...        ║"
    echo "╚══════════════════════════════════════════════╝"
    exit 1
  fi
fi

# Build Rust core if needed
BINARY="$SCRIPT_DIR/baoclaw-core/target/release/baoclaw-core"
if [ ! -f "$BINARY" ]; then
  echo "🔨 Building Rust core (first time, may take a minute)..."
  cd "$SCRIPT_DIR/baoclaw-core"
  cargo build --release 2>&1 | tail -3
  cd "$SCRIPT_DIR"
  echo "✓ Build complete"
fi

# Install TS deps if needed
if [ ! -d "$SCRIPT_DIR/ts-ipc/node_modules" ]; then
  echo "📦 Installing dependencies..."
  cd "$SCRIPT_DIR/ts-ipc"
  npm install --silent
  cd "$SCRIPT_DIR"
fi

# Launch
export BAOCLAW_CORE_BIN="$BINARY"
cd "$SCRIPT_DIR"
npx --prefix ts-ipc tsx ts-ipc/cli.ts
