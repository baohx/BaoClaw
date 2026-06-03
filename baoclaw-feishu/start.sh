#!/bin/bash
# BaoClaw Feishu Gateway — startup script
# Usage:
#   ./start.sh              foreground (Ctrl+C to stop)
#   ./start.sh --daemon     background daemon
#   ./start.sh --debug      verbose logging
#   ./start.sh --stop       kill running daemon
#   ./start.sh --status     check if running
#   ./start.sh --logs       tail the log file
set -e
cd "$(dirname "$0")"

PID_FILE="$HOME/.baoclaw-feishu.pid"
LOG_FILE="$HOME/.baoclaw/logs/baoclaw-feishu.log"

# ── Commands ──

case "${1:-}" in
  --stop)
    if [ -f "$PID_FILE" ]; then
      PID=$(cat "$PID_FILE")
      if kill -0 "$PID" 2>/dev/null; then
        echo "[feishu] Stopping (PID=$PID)..."
        kill "$PID"
        rm -f "$PID_FILE"
        echo "[feishu] ✅ Stopped"
      else
        echo "[feishu] PID $PID not running — removing stale PID file"
        rm -f "$PID_FILE"
      fi
    else
      echo "[feishu] No PID file found — not running"
    fi
    exit 0
    ;;
  --status)
    if [ -f "$PID_FILE" ]; then
      PID=$(cat "$PID_FILE")
      if kill -0 "$PID" 2>/dev/null; then
        echo "[feishu] ✅ Running (PID=$PID)"
        echo "[feishu] Log: $LOG_FILE"
        exit 0
      else
        echo "[feishu] ⚠️ PID file exists but process dead"
        exit 1
      fi
    else
      echo "[feishu] ❌ Not running"
      exit 1
    fi
    ;;
  --logs)
    if [ -f "$LOG_FILE" ]; then
      tail -f "$LOG_FILE"
    else
      echo "[feishu] No log file yet"
    fi
    exit 0
    ;;
  --restart)
    "$0" --stop 2>/dev/null
    sleep 1
    exec "$0" "$2"
    ;;
esac

# ── Prerequisites ──

# Check daemon
if ! ls /tmp/baoclaw-sockets/*.json 2>/dev/null | head -1 | grep -q .; then
  echo "[feishu] ❌ No BaoClaw daemon found"
  echo "[feishu]    Start one with: baoclaw daemon &"
  echo "[feishu]    Or: cd ~/BaoClaw/baoclaw-core && cargo run -- daemon &"
  exit 1
fi

# Switch lark-cli profile
echo "[feishu] Switching to baoclaw profile..."
lark-cli profile use baoclaw >/dev/null 2>&1

# Restore profile on exit
ORIGINAL_PROFILE=$(lark-cli profile list --json 2>/dev/null | python3 -c "import sys,json; ps=[p for p in json.load(sys.stdin) if p.get('active')]; print(ps[0]['name'] if ps else 'cli_aa93ecb8aab9dbc1')" 2>/dev/null || echo "cli_aa93ecb8aab9dbc1")

cleanup() {
  echo ""
  echo "[feishu] Switching back to $ORIGINAL_PROFILE..."
  lark-cli profile use "$ORIGINAL_PROFILE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# ── Launch ──

FLAGS=""
for arg in "$@"; do
  case "$arg" in
    --debug|-d) FLAGS="$FLAGS --debug" ;;
    --daemon)   FLAGS="$FLAGS --daemon" ;;
  esac
done

echo "[feishu] Starting gateway$FLAGS..."
echo "[feishu] Log: $LOG_FILE"

if echo "$@" | grep -q -- "--daemon"; then
  # Background mode
  nohup npx tsx src/gateway.ts $FLAGS </dev/null >/dev/null 2>&1 &
  PID=$!
  echo "[feishu] Started daemon (PID=$PID)"
  echo "[feishu] Tail logs: ./start.sh --logs"
  echo "[feishu] Stop:       ./start.sh --stop"
  echo "[feishu] Status:     ./start.sh --status"
  # Don't wait — suppress trap output
  trap - EXIT
  disown $PID
else
  # Foreground mode
  exec npx tsx src/gateway.ts $FLAGS
fi
