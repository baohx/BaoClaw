/**
 * Daemon discovery over the well-known BaoClaw socket locations.
 *
 * Socket conventions (kept in sync with baoclaw-core/src/main.rs):
 *   Linux:   $XDG_RUNTIME_DIR/baoclaw.sock (typically /run/user/<UID>/)
 *   macOS:   /tmp/baoclaw-sockets/baoclaw.sock
 *   Windows: %TEMP%/baoclaw-sockets/baoclaw.sock
 */
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { IpcClient } from "./client.js";
import { createLogger } from "./logger.js";

const logger = createLogger("ts-ipc");

export interface DaemonInfo {
  pid: number;
  cwd: string;
  session_id: string;
  socket: string;
  started_at: string;
}

export interface DaemonConnectorOptions {
  /**
   * Fallback session id used when connecting without an explicit
   * shared session (e.g. 'feishu', 'whatsapp').
   */
  sessionTag?: string;
}

export function getSocketDir(): string {
  return path.join(os.tmpdir(), "baoclaw-sockets");
}

export function resolveFixedSocket(): string | null {
  if (process.platform === "linux") {
    const runtimeDir = process.env.XDG_RUNTIME_DIR;
    return runtimeDir && fs.existsSync(runtimeDir)
      ? path.join(runtimeDir, "baoclaw.sock")
      : null;
  }
  return path.join(getSocketDir(), "baoclaw.sock");
}

/**
 * Select the most recently started daemon from a list.
 * Returns null if the list is empty.
 */
export function selectNewestDaemon(daemons: DaemonInfo[]): DaemonInfo | null {
  if (daemons.length === 0) return null;
  return daemons.reduce((newest, d) =>
    new Date(d.started_at).getTime() > new Date(newest.started_at).getTime()
      ? d
      : newest,
  );
}

export class DaemonConnector {
  private readonly sessionTag: string;
  private reconnectCountValue = 0;
  private lastDisconnectErrorValue: Error | null = null;
  private lastConnectAtValue: Date | null = null;
  get reconnectCount(): number {
    return this.reconnectCountValue;
  }
  get lastDisconnectError(): Error | null {
    return this.lastDisconnectErrorValue;
  }
  get lastConnectAt(): Date | null {
    return this.lastConnectAtValue;
  }

  constructor(options: DaemonConnectorOptions = {}) {
    this.sessionTag = options.sessionTag ?? "default";
  }

  /**
   * Discover running BaoClaw daemon instances by scanning metadata files.
   */
  discover(): DaemonInfo[] {
    const dir = getSocketDir();
    if (!fs.existsSync(dir)) return [];

    const daemons: DaemonInfo[] = [];
    for (const file of fs.readdirSync(dir)) {
      if (!file.endsWith(".json")) continue;
      try {
        const meta: DaemonInfo = JSON.parse(
          fs.readFileSync(path.join(dir, file), "utf-8"),
        );
        // Check if the process is still alive
        try {
          process.kill(meta.pid, 0);
        } catch {
          continue; // dead process
        }
        // Check if socket file exists
        if (!fs.existsSync(meta.socket)) continue;
        daemons.push(meta);
      } catch {
        /* skip invalid files */
      }
    }
    return daemons;
  }

  /**
   * Connect to a daemon via UDS and send initialize.
   * Optionally passes sharedSessionId for session affinity.
   */
  async connect(
    info: DaemonInfo,
    sharedSessionId?: string,
  ): Promise<IpcClient> {
    const client = new IpcClient({ requestTimeoutMs: 0 });
    await client.connect(info.socket);
    this.lastConnectAtValue = new Date();
    client.onDisconnect((error) => {
      this.reconnectCountValue++;
      this.lastDisconnectErrorValue = error;
    });
    const initParams: Record<string, unknown> = {
      cwd: info.cwd,
      settings: {},
    };
    if (sharedSessionId) {
      initParams.shared_session_id = sharedSessionId;
    }
    await client.request("initialize", initParams);
    return client;
  }

  /**
   * Discover and connect to the newest daemon.
   * Retries every retryIntervalMs for up to maxWaitMs.
   * Optionally passes sharedSessionId for session affinity.
   */
  async discoverAndConnect(
    maxWaitMs: number = 60_000,
    retryIntervalMs: number = 5_000,
    sharedSessionId?: string,
  ): Promise<{ client: IpcClient; info: DaemonInfo }> {
    const deadline = Date.now() + maxWaitMs;
    let lastError: Error | null = null;

    while (Date.now() < deadline) {
      const fixedSocket = resolveFixedSocket();
      if (fixedSocket && fs.existsSync(fixedSocket)) {
        const fixedInfo: DaemonInfo = {
          pid: 0,
          cwd: process.cwd(),
          session_id: sharedSessionId ?? this.sessionTag,
          socket: fixedSocket,
          started_at: new Date().toISOString(),
        };
        try {
          const client = await this.connect(fixedInfo, sharedSessionId);
          return { client, info: fixedInfo };
        } catch (err: any) {
          lastError = err instanceof Error ? err : new Error(String(err));
          logger.warn(
            `Fixed daemon connect failed (socket=${fixedSocket}): ${lastError.message}`,
          );
        }
      }
      const daemons = this.discover();
      const best = selectNewestDaemon(daemons);
      if (best) {
        try {
          const client = await this.connect(best, sharedSessionId);
          return { client, info: best };
        } catch (err: any) {
          lastError = err instanceof Error ? err : new Error(String(err));
          logger.warn(
            `Daemon connect failed (pid=${best.pid}, socket=${best.socket}): ${lastError.message}`,
          );
        }
      }
      await sleep(retryIntervalMs);
    }

    const tail = lastError ? ` Last error: ${lastError.message}` : "";
    throw new Error(
      `No BaoClaw daemon found after ${maxWaitMs / 1000}s. Start one with: baoclaw.${tail}`,
    );
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
