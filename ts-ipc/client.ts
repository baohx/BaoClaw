import * as net from 'net';

interface JsonRpcRequest {
  jsonrpc: '2.0';
  method: string;
  params?: unknown;
  id: number | string;
}

interface JsonRpcNotification {
  jsonrpc: '2.0';
  method: string;
  params?: unknown;
}

type NotificationHandler = (params: unknown) => void;

export class IpcClient {
  private socket: net.Socket | null = null;
  private buffer = '';
  private nextId = 1;
  private pendingRequests = new Map<
    number | string,
    {
      resolve: (value: unknown) => void;
      reject: (error: Error) => void;
      timer: ReturnType<typeof setTimeout>;
    }
  >();
  private notificationHandlers = new Map<string, NotificationHandler[]>();

  /**
   * Connect to a Unix Domain Socket at the given path.
   */
  async connect(socketPath: string): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      const socket = net.createConnection(socketPath, () => {
        this.socket = socket;
        resolve();
      });

      socket.on('data', (data: Buffer) => this.handleData(data));

      socket.on('error', (err: Error) => {
        if (!this.socket) {
          // Connection failed during initial connect
          reject(err);
        }
        // For established connections, errors will trigger 'close'
      });

      socket.on('close', () => {
        this.handleDisconnect(new Error('Connection closed'));
      });
    });
  }

  /**
   * Send a JSON-RPC 2.0 request and wait for the matching response.
   * @param method - The RPC method name
   * @param params - Optional parameters
   * @param timeoutMs - Request timeout in milliseconds (default 30s)
   * @returns The result field from the JSON-RPC response
   * @throws Error if the response contains an error, the request times out, or the connection is lost
   */
  async request<T = unknown>(method: string, params?: unknown, timeoutMs = 30000): Promise<T> {
    if (!this.socket) {
      throw new Error('Not connected');
    }

    const id = this.nextId++;
    const request: JsonRpcRequest = {
      jsonrpc: '2.0',
      method,
      ...(params !== undefined && { params }),
      id,
    };

    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingRequests.delete(id);
        reject(new Error(`Request ${method} (id=${id}) timed out after ${timeoutMs}ms`));
      }, timeoutMs);

      this.pendingRequests.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
        timer,
      });

      const line = JSON.stringify(request) + '\n';
      this.socket!.write(line);
    });
  }

  /**
   * Send a JSON-RPC 2.0 notification (fire-and-forget, no response expected).
   */
  notify(method: string, params?: unknown): void {
    if (!this.socket) {
      throw new Error('Not connected');
    }

    const notification: JsonRpcNotification = {
      jsonrpc: '2.0',
      method,
      ...(params !== undefined && { params }),
    };

    const line = JSON.stringify(notification) + '\n';
    this.socket.write(line);
  }

  /**
   * Register a handler for notifications with the given method name.
   * @returns An unsubscribe function that removes this handler
   */
  onNotification(method: string, handler: NotificationHandler): () => void {
    const handlers = this.notificationHandlers.get(method) ?? [];
    handlers.push(handler);
    this.notificationHandlers.set(method, handlers);

    return () => {
      const current = this.notificationHandlers.get(method);
      if (current) {
        const idx = current.indexOf(handler);
        if (idx !== -1) {
          current.splice(idx, 1);
        }
        if (current.length === 0) {
          this.notificationHandlers.delete(method);
        }
      }
    };
  }

  /**
   * Gracefully disconnect from the socket.
   * Rejects all pending requests and cleans up resources.
   */
  async disconnect(): Promise<void> {
    return new Promise<void>((resolve) => {
      this.handleDisconnect(new Error('Client disconnected'));

      if (this.socket) {
        const socket = this.socket;
        this.socket = null;
        socket.end(() => resolve());
      } else {
        resolve();
      }
    });
  }

  /**
   * Handle incoming data from the socket.
   * Implements NDJSON framing: buffer data, split by newlines,
   * process complete lines, keep partial lines in buffer.
   */
  private handleData(data: Buffer): void {
    this.buffer += data.toString('utf-8');

    let newlineIdx: number;
    while ((newlineIdx = this.buffer.indexOf('\n')) !== -1) {
      const line = this.buffer.slice(0, newlineIdx).trim();
      this.buffer = this.buffer.slice(newlineIdx + 1);

      if (line.length > 0) {
        this.handleMessage(line);
      }
    }
  }

  /**
   * Parse a complete JSON line and route it to the appropriate handler:
   * - If it has an 'id' field, it's a response to a pending request
   * - If it has a 'method' field (no 'id'), it's a notification
   */
  private handleMessage(json: string): void {
    let parsed: Record<string, unknown>;
    try {
      parsed = JSON.parse(json);
    } catch {
      // Ignore malformed JSON lines
      return;
    }

    // Check if this is a response (has 'id' and either 'result' or 'error')
    if ('id' in parsed && parsed.id != null) {
      const id = parsed.id as number | string;
      const pending = this.pendingRequests.get(id);
      if (pending) {
        this.pendingRequests.delete(id);
        clearTimeout(pending.timer);

        if ('error' in parsed && parsed.error) {
          const err = parsed.error as { code: number; message: string; data?: unknown };
          const error = new Error(err.message);
          (error as Error & { code: number; data?: unknown }).code = err.code;
          (error as Error & { code: number; data?: unknown }).data = err.data;
          pending.reject(error);
        } else {
          pending.resolve(parsed.result);
        }
      }
      return;
    }

    // Otherwise it's a notification (has 'method', no 'id')
    if ('method' in parsed && typeof parsed.method === 'string') {
      const handlers = this.notificationHandlers.get(parsed.method);
      if (handlers) {
        for (const handler of handlers) {
          try {
            handler(parsed.params);
          } catch {
            // Notification handlers should not throw, but don't let one break others
          }
        }
      }
    }
  }

  /**
   * Clean up all pending requests on disconnect.
   */
  private handleDisconnect(error: Error): void {
    for (const [, pending] of this.pendingRequests) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pendingRequests.clear();
  }
}
