#!/bin/bash
# BaoClaw Web — browser-based chat interface
# Usage: baoclaw-web [--port 8080]
BAOCLAW_HOME="${BAOCLAW_HOME:-$HOME/.baoclaw}"
exec npx --prefix "$BAOCLAW_HOME/baoclaw-web" tsx "$BAOCLAW_HOME/baoclaw-web/src/server.ts" "$@"
