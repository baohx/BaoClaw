#!/usr/bin/env node
import React from "react";
import { render } from "ink";
import { App } from "./components/App.js";
import { createIpcConnection } from "./ipc.js";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";

/**
 * Resolve the daemon socket path.
 * Priority: explicit arg > fixed socket (XDG_RUNTIME_DIR or /tmp) > legacy paths.
 * This mirrors the CLI's resolveDaemonSocket() logic.
 */
function resolveDaemonSocket(): string {
  // 1. Explicit argument
  if (process.argv[2]) return process.argv[2];

  // 2. Fixed socket path (matches daemon's P3-1c logic)
  //    Linux: $XDG_RUNTIME_DIR/baoclaw.sock
  //    macOS: /tmp/baoclaw.sock
  const xdgRuntime = process.env.XDG_RUNTIME_DIR;
  if (xdgRuntime) {
    const fixed = path.join(xdgRuntime, "baoclaw.sock");
    if (fs.existsSync(fixed)) return fixed;
  }

  // 3. macOS default
  const macDefault = "/tmp/baoclaw.sock";
  if (fs.existsSync(macDefault)) return macDefault;

  // 4. Legacy fallback (old cwd-hash or PID-based sockets)
  const tmpDir = os.tmpdir();
  const candidates = fs
    .readdirSync(tmpDir)
    .filter((f) => f.startsWith("baoclaw") && f.endsWith(".sock"))
    .map((f) => ({ f, mtime: fs.statSync(path.join(tmpDir, f)).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime);

  if (candidates.length > 0) {
    return path.join(tmpDir, candidates[0].f);
  }

  // 5. Final fallback: XDG or /tmp
  return xdgRuntime ? path.join(xdgRuntime, "baoclaw.sock") : macDefault;
}

async function main() {
  // Resolve socket path (matches CLI logic)
  const socketPath = resolveDaemonSocket();

  // Get model from config (prefer model_profiles over legacy model field)
  let model = "unknown";
  try {
    const configPath = path.join(os.homedir(), ".baoclaw", "config.json");
    if (fs.existsSync(configPath)) {
      const config = JSON.parse(fs.readFileSync(configPath, "utf-8"));
      // New format: model_profiles + primary_profile
      if (config.primary_profile && config.model_profiles) {
        const primary = config.model_profiles[config.primary_profile];
        if (primary && primary.model) {
          model = primary.model;
        }
      }
      // Legacy fallback
      if (model === "unknown") {
        model = config.model || config.defaultModel || "unknown";
      }
    }
  } catch (err) {
    // Ignore config read errors
  }

  // Connect to backend
  console.log(`Connecting to BaoClaw at ${socketPath}...`);

  try {
    const client = await createIpcConnection({ socketPath });
    console.log("Connected!");

    // Render TUI
    render(React.createElement(App, { client, model }));
  } catch (err) {
    console.error("Failed to connect:", err);
    console.error(
      "\nMake sure BaoClaw is running and the socket path is correct.",
    );
    console.error("Usage: baoclaw-tui [socket-path]");
    console.error("\nTo start the daemon:");
    console.error(
      "  systemctl --user start baoclaw   # or: baoclaw --daemon &",
    );
    process.exit(1);
  }
}

main();
