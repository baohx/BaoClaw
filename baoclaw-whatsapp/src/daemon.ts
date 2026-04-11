/**
 * Daemon discovery and connection.
 * Scans /tmp/baoclaw-sockets/ for running BaoClaw daemon instances,
 * selects the most recently started one, and connects via UDS.
 */
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { IpcClient } from './ipcClient.js';

export interface DaemonInfo {
  pid: number;
  cwd: string;
  session_id: string;
  socket: string;
  started_at: string;
}

function getSocketDir(): string {
  return path.join(os.tmpdir(), 'baoclaw-sockets');
}

/**
 * Select the most recently started daemon from a list.
 * Returns null if the list is empty.
 */
export function selectNewestDaemon(daemons: DaemonInfo[]): DaemonInfo | null {
  if (daemons.length === 0) return null;
  return daemons.reduce((newest, d) =>
    new Date(d.started_at).getTime() > new Date(newest.started_at).getTime() ? d : newest
  );
}

export class DaemonConnector {
  /**
   * Discover running BaoClaw daemon instances by scanning metadata files.
   */
  discover(): DaemonInfo[] {
    const dir = getSocketDir();
    if (!fs.existsSync(dir)) return [];

    const daemons: DaemonInfo[] = [];
    for (const file of fs.readdirSync(dir)) {
      if (!file.endsWith('.json')) continue;
      try {
        const meta: DaemonInfo = JSON.parse(
          fs.readFileSync(path.join(dir, file), 'utf-8'),
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
   */
  async connect(info: DaemonInfo): Promise<IpcClient> {
    const client = new IpcClient();
    await client.connect(info.socket);
    await client.request('initialize', { cwd: info.cwd });
    return client;
  }

  /**
   * Discover and connect to the newest daemon.
   * Retries every retryIntervalMs for up to maxWaitMs.
   */
  async discoverAndConnect(
    maxWaitMs: number = 60_000,
    retryIntervalMs: number = 5_000,
  ): Promise<{ client: IpcClient; info: DaemonInfo }> {
    const deadline = Date.now() + maxWaitMs;

    while (Date.now() < deadline) {
      const daemons = this.discover();
      const best = selectNewestDaemon(daemons);
      if (best) {
        try {
          const client = await this.connect(best);
          return { client, info: best };
        } catch {
          // Connection failed, will retry
        }
      }
      await sleep(retryIntervalMs);
    }

    throw new Error(
      `No BaoClaw daemon found after ${maxWaitMs / 1000}s. Start one with: baoclaw`,
    );
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
