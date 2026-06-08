/**
 * IPC 客户端
 * 
 * 与 baoclaw-core daemon 通信
 */
import * as net from 'net';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

export interface IpcClient {
  connect(socketPath: string): Promise<void>;
  request<T = unknown>(method: string, params?: unknown): Promise<T>;
  onEvent(handler: (event: Record<string, unknown>) => void): void;
  disconnect(): Promise<void>;
}

export class IpcClientImpl implements IpcClient {
  private socket: net.Socket | null = null;
  private buffer = '';
  private nextId = 1;
  private pending = new Map<number, { 
    resolve: (v: unknown) => void; 
    reject: (e: Error) => void;
  }>();
  private eventHandlers: ((event: Record<string, unknown>) => void)[] = [];
  
  async connect(socketPath: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const sock = net.createConnection(socketPath, () => {
        this.socket = sock;
        resolve();
      });
      
      sock.on('data', (data: Buffer) => this.onData(data));
      sock.on('error', (err) => {
        if (!this.socket) reject(err);
      });
      sock.on('close', () => this.onClose());
    });
  }
  
  async request<T = unknown>(method: string, params?: unknown): Promise<T> {
    if (!this.socket) throw new Error('Not connected');
    
    const id = this.nextId++;
    const msg: Record<string, unknown> = { jsonrpc: '2.0', method, id };
    if (params !== undefined) msg.params = params;
    
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.socket!.write(JSON.stringify(msg) + '\n');
    });
  }
  
  onEvent(handler: (event: Record<string, unknown>) => void): void {
    this.eventHandlers.push(handler);
  }
  
  async disconnect(): Promise<void> {
    if (this.socket) {
      this.socket.end();
      this.socket = null;
    }
  }
  
  private onData(data: Buffer) {
    this.buffer += data.toString('utf-8');
    let idx: number;
    
    while ((idx = this.buffer.indexOf('\n')) !== -1) {
      const line = this.buffer.slice(0, idx).trim();
      this.buffer = this.buffer.slice(idx + 1);
      if (line) this.handleLine(line);
    }
  }
  
  private handleLine(json: string) {
    let p: Record<string, unknown>;
    try {
      p = JSON.parse(json);
    } catch {
      return;
    }
    
    // 响应
    if ('id' in p && p.id != null) {
      const req = this.pending.get(p.id as number);
      if (req) {
        this.pending.delete(p.id as number);
        if ('error' in p) {
          req.reject(new Error((p.error as { message: string }).message));
        } else {
          req.resolve(p.result);
        }
      }
      return;
    }
    
    // 事件通知
    if ('method' in p && p.method === 'stream/event') {
      const event = p.params as Record<string, unknown>;
      for (const handler of this.eventHandlers) {
        try {
          handler(event);
        } catch (err) {
          console.error('Event handler error:', err);
        }
      }
    }
  }
  
  private onClose() {
    for (const [, p] of this.pending) {
      p.reject(new Error('Connection closed'));
    }
    this.pending.clear();
  }
}

// 工厂函数
export function createIpcClient(): IpcClient {
  return new IpcClientImpl();
}

// 发现 daemon socket
export function discoverDaemonSocket(): string | null {
  const socketDir = path.join(os.tmpdir(), 'baoclaw-sockets');
  
  if (!fs.existsSync(socketDir)) return null;
  
  const files = fs.readdirSync(socketDir);
  for (const file of files) {
    if (!file.endsWith('.json')) continue;
    
    try {
      const meta = JSON.parse(fs.readFileSync(path.join(socketDir, file), 'utf-8'));
      if (meta.socket && fs.existsSync(meta.socket)) {
        // 检查进程是否存活
        try {
          process.kill(meta.pid, 0);
          return meta.socket;
        } catch {
          // 进程已死
        }
      }
    } catch {
      // 跳过无效文件
    }
  }
  
  return null;
}
