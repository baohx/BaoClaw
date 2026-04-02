#!/bin/bash
# BaoClaw — Launch Script
# Usage: ANTHROPIC_API_KEY=sk-ant-... ./start.sh
#        ANTHROPIC_API_KEY=key ANTHROPIC_BASE_URL=https://proxy.example.com ./start.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Check API key
if [ -z "$ANTHROPIC_API_KEY" ]; then
  echo "╔══════════════════════════════════════════════╗"
  echo "║  ANTHROPIC_API_KEY is not set.               ║"
  echo "║                                              ║"
  echo "║  Usage:                                      ║"
  echo "║    export ANTHROPIC_API_KEY=sk-ant-...       ║"
  echo "║    ./start.sh                                ║"
  echo "║                                              ║"
  echo "║  For custom API endpoint:                    ║"
  echo "║    export ANTHROPIC_BASE_URL=https://...     ║"
  echo "╚══════════════════════════════════════════════╝"
  exit 1
fi

# Build Rust core if needed
BINARY="$SCRIPT_DIR/claude-core/target/release/claude-core"
if [ ! -f "$BINARY" ]; then
  echo "🔨 Building Rust core (first time, may take a minute)..."
  cd "$SCRIPT_DIR/claude-core"
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
export CLAUDE_CORE_BIN="$BINARY"
cd "$SCRIPT_DIR"
npx --prefix ts-ipc tsx ts-ipc/cli.ts
