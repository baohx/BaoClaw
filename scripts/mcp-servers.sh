#!/bin/bash
# BaoClaw MCP Server Launcher
# Manages MCP servers defined in ~/.baoclaw/mcp.json
#
# IMPORTANT: stdio-type MCP servers CANNOT be started with nohup/background!
#   They require stdin/stdout pipes to communicate with the daemon.
#   This script only manages SSE/HTTP-type servers. stdio servers are
#   spawned on-demand by the daemon itself.
#
# Usage: ./mcp-servers.sh [start|stop|restart|status|debug]

set -e

MCP_CONFIG="$HOME/.baoclaw/mcp.json"
LOG_DIR="$HOME/.baoclaw/logs"
PID_DIR="$HOME/.baoclaw/pids"

# Create directories
mkdir -p "$LOG_DIR" "$PID_DIR"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"
}

# Check if a server uses stdio transport (cannot be backgrounded)
is_stdio_server() {
    local name="$1"
    local transport
    transport=$(jq -r ".mcpServers.\"$name\".transport // \"stdio\"" "$MCP_CONFIG" 2>/dev/null)

    # transport can be:
    # - "stdio" (string) → stdio
    # - "sse" (string) → SSE
    # - { "sse": { ... } } (object) → SSE
    # - default (missing) → stdio (MCP default)
    if [ "$transport" = "stdio" ] || [ "$transport" = "null" ] || [ -z "$transport" ]; then
        return 0  # true = is stdio
    else
        return 1  # false = is SSE/HTTP
    fi
}

# Parse mcp.json and start servers
start_servers() {
    if [ ! -f "$MCP_CONFIG" ]; then
        log "ERROR: MCP config not found: $MCP_CONFIG"
        exit 1
    fi

    # Check if jq is available
    if ! command -v jq &> /dev/null; then
        log "ERROR: jq is required but not installed"
        exit 1
    fi

    # Get list of server names
    local server_names
    server_names=$(jq -r '.mcpServers | keys[]' "$MCP_CONFIG" 2>/dev/null) || {
        log "ERROR: Failed to parse mcp.json"
        exit 1
    }

    local stdio_count=0
    local sse_started=0

    while IFS= read -r name; do
        [ -z "$name" ] && continue

        # Skip stdio servers — they must be spawned by daemon
        if is_stdio_server "$name"; then
            log "SKIP (stdio): $name — managed by daemon, not this script"
            stdio_count=$((stdio_count + 1))
            continue
        fi

        # --- SSE/HTTP server: safe to background ---
        local command
        command=$(jq -r ".mcpServers.\"$name\".command // empty" "$MCP_CONFIG")

        local args_json
        args_json=$(jq -c ".mcpServers.\"$name\".args // []" "$MCP_CONFIG")

        [ -z "$command" ] && {
            log "WARN: $name has no command, skipping"
            continue
        }

        # Check if already running
        local existing_pids
        existing_pids=$(pgrep -f "$name" 2>/dev/null || true)
        if [ -n "$existing_pids" ]; then
            log "SKIP: $name already running (PIDs: $existing_pids)"
            continue
        fi

        log "Starting: $name (SSE/HTTP)"

        # Build the command
        local cmd
        if [ "$command" = "uvx" ]; then
            local uvx_args
            uvx_args=$(jq -r '.[]' <<< "$args_json" | tr '\n' ' ')
            cmd="uvx $uvx_args"
        elif [ "$command" = "uv" ]; then
            local uv_dir
            uv_dir=$(jq -r '.[]' <<< "$args_json" | grep "\-\-directory" -A1 | tail -1)
            if [ -d "$uv_dir/.venv" ]; then
                cmd="bash -c 'cd $uv_dir && source .venv/bin/activate && python server.py'"
            else
                local uv_run_args
                uv_run_args=$(jq -r '.[]' <<< "$args_json" | tr '\n' ' ')
                cmd="uv $uv_run_args"
            fi
        else
            local custom_args
            custom_args=$(jq -r '.[]' <<< "$args_json" | tr '\n' ' ')
            if [ -n "$custom_args" ]; then
                cmd="$command $custom_args"
            else
                cmd="$command"
            fi
        fi

        log "  Command: $cmd"

        # Start SSE/HTTP server in background (safe — no stdin needed)
        nohup setsid bash -c "$cmd" >> "$LOG_DIR/$name.log" 2>&1 < /dev/null &

        sleep 2

        local new_pids
        new_pids=$(pgrep -f "$name" 2>/dev/null || true)
        if [ -n "$new_pids" ]; then
            log "  ✓ $name running (PIDs: $new_pids)"
            echo "$new_pids" | cut -d' ' -f1 > "$PID_DIR/$name.pid"
            sse_started=$((sse_started + 1))
        else
            log "  ✗ $name FAILED to start - check log: $LOG_DIR/$name.log"
        fi

    done <<< "$server_names"

    log "Done: $sse_started SSE/HTTP server(s) started, $stdio_count stdio server(s) deferred to daemon"
    if [ $stdio_count -gt 0 ]; then
        log ""
        log "NOTE: stdio MCP servers are automatically spawned by the BaoClaw daemon"
        log "      when you start a conversation. No manual action needed."
        log "      To verify, run: baoclaw → /mcp"
    fi
}

stop_servers() {
    local killed=0

    # Kill by PID files
    for pidfile in "$PID_DIR"/*.pid; do
        [ -f "$pidfile" ] || continue
        local name
        name=$(basename "$pidfile" .pid)
        local pid
        pid=$(cat "$pidfile" | cut -d' ' -f1)

        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            log "Stopping: $name (PID: $pid)"
            kill "$pid" 2>/dev/null || true
            killed=1
        fi
        rm -f "$pidfile"
    done

    # Also kill any remaining SSE/HTTP MCP processes by name pattern
    # (stdio servers are managed by daemon, don't kill them here)
    local patterns=("computer-control-mcp" "excalidraw-architect-mcp")
    for pattern in "${patterns[@]}"; do
        pkill -f "$pattern" 2>/dev/null || true
    done

    if [ $killed -eq 1 ]; then
        sleep 1
        for pattern in "${patterns[@]}"; do
            pkill -9 -f "$pattern" 2>/dev/null || true
        done
    fi

    log "All managed MCP servers stopped"
}

status_servers() {
    echo "MCP Server Status:"
    echo "=================="
    echo ""

    # Show config overview
    if [ -f "$MCP_CONFIG" ]; then
        echo "Configured servers (from mcp.json):"
        jq -r '.mcpServers | keys[]' "$MCP_CONFIG" 2>/dev/null | while read name; do
            local transport="stdio"
            if ! is_stdio_server "$name"; then
                transport="sse/http"
            fi
            echo "  • $name ($transport)"
        done
        echo ""
    fi

    echo "Managed (SSE/HTTP) servers:"
    local found=0
    for pidfile in "$PID_DIR"/*.pid; do
        [ -f "$pidfile" ] || continue
        found=1
        local name
        name=$(basename "$pidfile" .pid)
        local pid
        pid=$(cat "$pidfile" | cut -d' ' -f1)

        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            echo "  ✓ $name (PID: $pid) - RUNNING"
        else
            echo "  ✗ $name (was PID: $pid) - NOT RUNNING"
            rm -f "$pidfile"
        fi
    done
    if [ $found -eq 0 ]; then
        echo "  (no SSE/HTTP servers started)"
    fi

    echo ""
    echo "stdio servers (spawned by daemon on demand):"
    echo "  Use '/mcp' inside baoclaw CLI/TUI to see live status"
}

debug_servers() {
    echo "=== MCP Config ==="
    cat "$MCP_CONFIG"
    echo ""
    echo "=== Log Directory ==="
    ls -la "$LOG_DIR"
    echo ""
    echo "=== PID Directory ==="
    ls -la "$PID_DIR"
    echo ""
    echo "=== Running Processes ==="
    ps aux | grep -E "computer-control|excalidraw|glm-vision|mcp" | grep -v grep || echo "(none)"
    echo ""
    echo "=== Log Contents ==="
    for log in "$LOG_DIR"/*.log; do
        [ -f "$log" ] || continue
        echo "--- $(basename "$log") ---"
        tail -20 "$log"
    done
}

case "${1:-start}" in
    start)
        log "Starting MCP servers..."
        start_servers
        ;;
    stop)
        log "Stopping MCP servers..."
        stop_servers
        ;;
    restart)
        log "Restarting MCP servers..."
        stop_servers
        sleep 2
        start_servers
        ;;
    status)
        status_servers
        ;;
    debug)
        debug_servers
        ;;
    *)
        echo "Usage: $0 {start|stop|restart|status|debug}"
        exit 1
        ;;
esac
