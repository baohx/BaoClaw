#!/bin/bash
# BaoClaw MCP Server Launcher
# Starts all MCP servers defined in ~/.baoclaw/mcp.json
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

    while IFS= read -r name; do
        [ -z "$name" ] && continue

        # Get command and args for this server
        local command
        command=$(jq -r ".mcpServers.\"$name\".command // empty" "$MCP_CONFIG")
        
        local args_json
        args_json=$(jq -c ".mcpServers.\"$name\".args // []" "$MCP_CONFIG")

        [ -z "$command" ] && continue

        # Check if already running
        local existing_pids
        existing_pids=$(pgrep -f "$name" 2>/dev/null || true)
        if [ -n "$existing_pids" ]; then
            log "SKIP: $name already running (PIDs: $existing_pids)"
            continue
        fi

        log "Starting: $name"

        # Build the command based on type
        local cmd
        if [ "$command" = "uvx" ]; then
            # uvx takes remaining args as package spec
            local uvx_args
            uvx_args=$(jq -r '.[]' <<< "$args_json" | tr '\n' ' ')
            cmd="uvx $uvx_args"
        elif [ "$command" = "uv" ]; then
            # Check if directory has existing .venv
            local uv_dir
            uv_dir=$(jq -r '.[]' <<< "$args_json" | grep "\-\-directory" -A1 | tail -1)
            if [ -d "$uv_dir/.venv" ]; then
                # Use existing venv instead of uv run
                cmd="bash -c 'cd $uv_dir && source .venv/bin/activate && python server.py'"
            else
                # uv run needs each arg separate
                local uv_run_args
                uv_run_args=$(jq -r '.[]' <<< "$args_json" | tr '\n' ' ')
                cmd="uv $uv_run_args"
            fi
        else
            # Custom command with args
            local custom_args
            custom_args=$(jq -r '.[]' <<< "$args_json" | tr '\n' ' ')
            if [ -n "$custom_args" ]; then
                cmd="$command $custom_args"
            else
                cmd="$command"
            fi
        fi

        log "  Command: $cmd"

        # Start the server in background using setsid to create new session
        # This prevents process from being killed when parent exits
        nohup setsid bash -c "$cmd" >> "$LOG_DIR/$name.log" 2>&1 &
        
        # Wait a bit for process to start
        sleep 2
        
        # Check if process is running
        local new_pids
        new_pids=$(pgrep -f "$name" 2>/dev/null || true)
        if [ -n "$new_pids" ]; then
            log "  ✓ $name running (PIDs: $new_pids)"
            # Save first PID
            echo "$new_pids" | cut -d' ' -f1 > "$PID_DIR/$name.pid"
        else
            log "  ✗ $name FAILED to start - check log: $LOG_DIR/$name.log"
        fi

    done <<< "$server_names"

    log "Done starting MCP servers"
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

    # Also kill any remaining MCP processes by name pattern
    local patterns=("computer-control" "excalidraw-architect" "glm-vision")
    for pattern in "${patterns[@]}"; do
        pkill -f "$pattern" 2>/dev/null || true
    done

    if [ $killed -eq 1 ]; then
        sleep 1
        # Force kill any that are still around
        for pattern in "${patterns[@]}"; do
            pkill -9 -f "$pattern" 2>/dev/null || true
        done
    fi

    log "All MCP servers stopped"
}

status_servers() {
    echo "MCP Server Status:"
    echo "=================="

    local found=0
    for pidfile in "$PID_DIR"/*.pid; do
        [ -f "$pidfile" ] || continue
        found=1
        local name
        name=$(basename "$pidfile" .pid)
        local pid
        pid=$(cat "$pidfile" | cut -d' ' -f1)

        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            echo "✓ $name (PID: $pid) - RUNNING"
        else
            echo "✗ $name (was PID: $pid) - NOT RUNNING"
            rm -f "$pidfile"
        fi
    done

    if [ $found -eq 0 ]; then
        echo "  (no PID files - no servers started yet)"
    fi

    echo ""
    echo "Running processes:"
    local count=0
    for name in computer-control excalidraw glm-vision; do
        pgrep -f "$name" | while read pid; do
            if [ -n "$pid" ]; then
                echo "  PID $pid: $(ps -p $pid -o comm= 2>/dev/null || echo 'unknown')"
                count=$((count + 1))
            fi
        done
    done
    if [ $count -eq 0 ]; then
        echo "  (none)"
    fi
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
    ps aux | grep -E "computer-control|excalidraw|glm-vision" | grep -v grep || echo "(none)"
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