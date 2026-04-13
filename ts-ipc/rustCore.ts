import { spawn, ChildProcess } from 'child_process';
import { IpcClient } from './client.js';

export interface RustCoreConfig {
  /** Path to the baoclaw-core binary */
  binaryPath: string;
  /** Working directory */
  cwd: string;
  /** Model to use */
  model?: string;
  /** Additional settings */
  settings?: Record<string, unknown>;
  /** Startup timeout in ms (default: 10000) */
  startupTimeoutMs?: number;
}

export interface RustCoreHandle {
  /** The IPC client connected to the Rust process */
  client: IpcClient;
  /** The capabilities returned by the initialize response */
  capabilities: Record<string, unknown>;
  /** The session ID */
  sessionId: string;
  /** Send shutdown request and wait for process exit */
  shutdown(): Promise<void>;
  /** Force kill the process */
  kill(): void;
}

/**
 * Read stdout of the child process line-by-line looking for the SOCKET:{path} line.
 * Returns the socket path, or rejects on timeout / process exit.
 */
function waitForSocketPath(
  child: ChildProcess,
  timeoutMs: number,
): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    let buffer = '';
    let resolved = false;

    const timer = setTimeout(() => {
      if (!resolved) {
        resolved = true;
        cleanup();
        child.kill();
        reject(new Error(`Rust core did not emit SOCKET: line within ${timeoutMs}ms`));
      }
    }, timeoutMs);

    const onData = (data: Buffer) => {
      buffer += data.toString('utf-8');
      let newlineIdx: number;
      while ((newlineIdx = buffer.indexOf('\n')) !== -1) {
        const line = buffer.slice(0, newlineIdx).trim();
        buffer = buffer.slice(newlineIdx + 1);
        if (line.startsWith('SOCKET:')) {
          const socketPath = line.slice('SOCKET:'.length).trim();
          if (socketPath.length > 0 && !resolved) {
            resolved = true;
            cleanup();
            resolve(socketPath);
            return;
          }
        }
      }
    };

    const onError = (err: Error) => {
      if (!resolved) {
        resolved = true;
        cleanup();
        reject(new Error(`Rust core process error: ${err.message}`));
      }
    };

    const onClose = (code: number | null) => {
      if (!resolved) {
        resolved = true;
        cleanup();
        reject(new Error(`Rust core exited with code ${code} before emitting SOCKET: line`));
      }
    };

    function cleanup() {
      clearTimeout(timer);
      child.stdout?.off('data', onData);
      child.off('error', onError);
      child.off('close', onClose);
    }

    child.stdout?.on('data', onData);
    child.on('error', onError);
    child.on('close', onClose);
  });
}

/**
 * Start the Rust core process, connect via IPC, and initialize.
 *
 * Flow:
 * 1. Spawn the baoclaw-core binary as a child process
 * 2. Read stdout for the SOCKET:{path} line (with timeout)
 * 3. Connect IPC client to the socket
 * 4. Send initialize request
 * 5. Return the handle
 */
export async function startRustCore(config: RustCoreConfig): Promise<RustCoreHandle> {
  const timeoutMs = config.startupTimeoutMs ?? 10000;

  // Step 1: Spawn the Rust binary
  const child = spawn(config.binaryPath, [], {
    cwd: config.cwd,
    stdio: ['pipe', 'pipe', 'pipe'],
    env: process.env,
  });

  // Collect stderr for diagnostics
  let stderrOutput = '';
  child.stderr?.on('data', (data: Buffer) => {
    stderrOutput += data.toString('utf-8');
  });

  // Step 2: Wait for SOCKET:{path} on stdout
  let socketPath: string;
  try {
    socketPath = await waitForSocketPath(child, timeoutMs);
  } catch (err) {
    // Ensure process is killed on failure
    if (!child.killed) {
      child.kill();
    }
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(
      stderrOutput
        ? `${message}\nstderr: ${stderrOutput.trim()}`
        : message,
    );
  }

  // Step 3: Connect IPC client
  const client = new IpcClient();
  try {
    await client.connect(socketPath);
  } catch (err) {
    if (!child.killed) {
      child.kill();
    }
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(`Failed to connect to Rust core IPC socket: ${message}`);
  }

  // Step 4: Send initialize request
  let initResult: { capabilities: Record<string, unknown>; sessionId?: string };
  try {
    initResult = await client.request<{ capabilities: Record<string, unknown>; sessionId?: string }>(
      'initialize',
      {
        cwd: config.cwd,
        ...(config.model !== undefined && { model: config.model }),
        ...(config.settings !== undefined && { settings: config.settings }),
      },
    );
  } catch (err) {
    await client.disconnect();
    if (!child.killed) {
      child.kill();
    }
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(`Failed to initialize Rust core: ${message}`);
  }

  const capabilities = initResult.capabilities ?? {};
  const sessionId = initResult.sessionId ?? '';

  // Step 5: Build and return the handle
  let shutdownCalled = false;

  const handle: RustCoreHandle = {
    client,
    capabilities,
    sessionId,

    async shutdown(): Promise<void> {
      if (shutdownCalled) return;
      shutdownCalled = true;

      try {
        await client.request('shutdown', undefined, 5000);
      } catch {
        // Ignore errors during shutdown request — process may already be gone
      }

      await client.disconnect();

      // Wait for process to exit gracefully, or force kill after 3s
      await new Promise<void>((resolve) => {
        if (child.exitCode !== null) {
          resolve();
          return;
        }

        const forceKillTimer = setTimeout(() => {
          if (!child.killed) {
            child.kill('SIGKILL');
          }
          resolve();
        }, 3000);

        child.on('close', () => {
          clearTimeout(forceKillTimer);
          resolve();
        });
      });
    },

    kill(): void {
      shutdownCalled = true;
      if (!child.killed) {
        child.kill('SIGKILL');
      }
    },
  };

  return handle;
}

/**
 * Start the Rust core with automatic restart on crash.
 * Returns a handle that auto-restarts the process if it exits unexpectedly.
 */
export async function startRustCoreWithRestart(
  config: RustCoreConfig,
  onRestart?: (attempt: number) => void,
  maxRestarts: number = 3,
): Promise<RustCoreHandle> {
  let currentHandle = await startRustCore(config);
  let restartCount = 0;
  let intentionalShutdown = false;
  let restartInProgress = false;

  // Monitor the child process for unexpected exit by watching the IPC client
  // We detect crashes via the client's disconnect event and attempt restart
  function monitorProcess(): void {
    // Listen for unexpected disconnects on the IPC client's underlying connection.
    // When the Rust process crashes, the socket closes, which triggers the client's
    // pending requests to reject. We use a notification listener as a heartbeat proxy.
    const unsubscribe = currentHandle.client.onNotification('__internal_crash_detect__', () => {
      // This handler is never actually called — it's just a way to register
      // a listener. The real crash detection happens when the socket closes
      // and pending requests reject.
    });

    // We can't directly listen for child process exit from here since we don't
    // have access to the ChildProcess. Instead, we rely on the proxy pattern:
    // when the underlying process dies, the next request will fail, and the
    // proxy handle's methods will trigger a restart.
    void unsubscribe;
  }

  async function attemptRestart(): Promise<void> {
    if (intentionalShutdown || restartInProgress) return;
    if (restartCount >= maxRestarts) {
      throw new Error(
        `Rust core crashed ${restartCount} times, exceeding max restarts (${maxRestarts})`,
      );
    }

    restartInProgress = true;
    restartCount++;

    if (onRestart) {
      onRestart(restartCount);
    }

    try {
      currentHandle = await startRustCore(config);
      restartInProgress = false;
      monitorProcess();
    } catch (err) {
      restartInProgress = false;
      throw err;
    }
  }

  /**
   * Wrap an async operation so that if it fails due to a disconnected client,
   * we attempt a restart and retry the operation once.
   */
  async function withAutoRestart<T>(fn: () => Promise<T>): Promise<T> {
    try {
      return await fn();
    } catch (err) {
      if (intentionalShutdown) throw err;

      const message = err instanceof Error ? err.message : String(err);
      // Detect connection-related failures that indicate a crash
      if (
        message.includes('Connection closed') ||
        message.includes('Not connected') ||
        message.includes('EPIPE') ||
        message.includes('ECONNRESET')
      ) {
        await attemptRestart();
        return fn();
      }
      throw err;
    }
  }

  monitorProcess();

  // Return a proxy handle that delegates to the current active handle
  const proxyHandle: RustCoreHandle = {
    get client(): IpcClient {
      return currentHandle.client;
    },
    get capabilities(): Record<string, unknown> {
      return currentHandle.capabilities;
    },
    get sessionId(): string {
      return currentHandle.sessionId;
    },

    async shutdown(): Promise<void> {
      intentionalShutdown = true;
      await currentHandle.shutdown();
    },

    kill(): void {
      intentionalShutdown = true;
      currentHandle.kill();
    },
  };

  // Wrap the client's request method to support auto-restart
  const proxyRequest = async function <T = unknown>(
    method: string,
    params?: unknown,
    timeoutMs?: number,
  ): Promise<T> {
    return withAutoRestart(() => {
      // Always use the current handle's client in case of restart
      return currentHandle.client.request<T>(method, params, timeoutMs);
    });
  };

  // Override the client's request on the proxy to add auto-restart behavior
  Object.defineProperty(proxyHandle, 'client', {
    get() {
      const client = currentHandle.client;
      // Return a lightweight proxy that intercepts request() calls
      return new Proxy(client, {
        get(target, prop, receiver) {
          if (prop === 'request') {
            return proxyRequest;
          }
          return Reflect.get(target, prop, receiver);
        },
      });
    },
  });

  return proxyHandle;
}
