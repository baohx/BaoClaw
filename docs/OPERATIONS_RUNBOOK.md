# BaoClaw Operations Runbook

This guide is for developers and operators running BaoClaw from a clean
checkout. Commands target Linux and macOS; run the Linux commands inside WSL2
on Windows.

## Installation Verification

Install the prerequisites: Git, Rust 1.96 or newer, Node.js 24, npm, and
`jq` for state inspection.

```bash
./install.sh
baoclaw --version
```

Expected output is `baoclaw 2.1.0`. Start a smoke session with `baoclaw`, then
exit with `/exit`. No provider credential is required for the version check, but
an API key is required to send a model request.

## Daemon Restart

The daemon persists active sessions on graceful shutdown. Restart the service
and reconnect from the same project directory:

```bash
if [ "$(uname -s)" = "Darwin" ]; then
  launchctl kickstart -k "gui/$(id -u)/com.baoclaw.daemon"
else
  systemctl --user restart baoclaw
  systemctl --user is-active baoclaw
fi
```

Expected Linux output is `active`. On macOS verify with
`launchctl list | grep baoclaw`.

## Stale Socket Recovery

Do not delete a socket merely because it exists. First inspect the daemon:

```bash
if [ "$(uname -s)" = "Darwin" ]; then
  launchctl list | grep baoclaw
  ls -la /tmp/baoclaw-sockets/baoclaw.sock
else
  systemctl --user status baoclaw
  ls -la "${XDG_RUNTIME_DIR:-/tmp}/baoclaw.sock"
fi
```

If no daemon is running and the socket is stale, stop the service, remove only
the confirmed stale socket, and start it again:

```bash
if [ "$(uname -s)" = "Darwin" ]; then
  launchctl stop com.baoclaw.daemon
  rm -f /tmp/baoclaw-sockets/baoclaw.sock
else
  systemctl --user stop baoclaw
  rm -f "${XDG_RUNTIME_DIR:-/tmp}/baoclaw.sock"
  rm -f /tmp/baoclaw-sockets/baoclaw.sock
fi
if [ "$(uname -s)" = "Darwin" ]; then
  launchctl start com.baoclaw.daemon
else
  systemctl --user start baoclaw
fi
```

Expected output is a running service and a newly created socket. `IpcServer`
also probes existing sockets and refuses to replace a live daemon.

## Gateway Authorization

Configure only placeholder-free, real allowlist IDs in the private config.
Empty Feishu allowlists reject all inbound chats; Telegram and WhatsApp must
also be restricted to intended users/chats. Restart the gateway after changing
configuration and verify an authorized test message succeeds while an
unauthorized message receives no tool execution.

Never put bot tokens or API keys in this document, shell history, issues, or
test fixtures. Credentials are stored in `~/.baoclaw/config.json` and the local
WhatsApp auth directory; protect both with owner-only permissions.

## Cron Recovery

Cron state is stored in `~/.baoclaw/cron.json`. Inspect it without editing:

```bash
jq . ~/.baoclaw/cron.json
```

If the file is malformed, preserve it for diagnosis, restore the last valid
backup, and restart the daemon. Cron writes use temporary-file replacement so
an interrupted write leaves the previous target intact.

## Corrupted State

Stop the daemon before moving a damaged state file. Keep the original and
restore the newest valid backup:

```bash
if [ "$(uname -s)" = "Darwin" ]; then
  launchctl stop com.baoclaw.daemon
else
  systemctl --user stop baoclaw
fi
mv ~/.baoclaw/cron.json ~/.baoclaw/cron.json.corrupt
if [ "$(uname -s)" = "Darwin" ]; then
  launchctl start com.baoclaw.daemon
else
  systemctl --user start baoclaw
fi
```

Expected output is a running daemon with no recovered jobs from the renamed
file. Do not delete session files until a backup has been copied elsewhere.

## Sandbox Limitations

`--sandbox bwrap` and `--sandbox docker` provide stronger isolation when the
backend is installed. `--sandbox none` runs commands directly on the host and
must be treated as privileged. Unsupported or unknown modes fail rather than
silently selecting direct execution. Process-group cleanup for grandchildren
is not guaranteed after cancellation.

## Diagnostics

Collect only redacted diagnostics:

```bash
baoclaw --version
if [ "$(uname -s)" = "Darwin" ]; then
  launchctl list | grep baoclaw
else
  systemctl --user status baoclaw
  journalctl --user -u baoclaw --since "15 minutes ago" --no-pager
fi
```

Do not attach prompts, tool output, session files, provider responses, or
credentials. Local logs rotate at 5 MiB with three retained files.
