#!/bin/bash
# BaoClaw baoclaw-web — Web browser chat
BAOCLAW_HOME="${BAOCLAW_HOME:-$HOME/.baoclaw}"
export BAOCLAW_CORE_BIN="$BAOCLAW_HOME/bin/baoclaw-core"
exec npx --prefix "$BAOCLAW_HOME/baoclaw-web/src" tsx "$BAOCLAW_HOME/baoclaw-web/src/server.ts" "$@"
