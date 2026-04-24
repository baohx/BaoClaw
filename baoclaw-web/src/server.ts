/**
 * BaoClaw Web Gateway — HTTP + WebSocket server that bridges to the daemon via UDS IPC.
 * Usage: cd /your/project && baoclaw-web [--port 8080]
 */
import * as http from 'http';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import * as net from 'net';
import { WebSocketServer, WebSocket } from 'ws';

// ═══════════════════════════════════════════════════════════════
// IPC Client (same pattern as CLI/Telegram)
// ═══════════════════════════════════════════════════════════════
class IpcClient {
  private socket: net.Socket | null = null;
  private buffer = '';
  private nextId = 1;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private notifHandlers = new Map<string, ((params: unknown) => void)[]>();
  private closeHandlers: (() => void)[] = [];

  async connect(socketPath: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const sock = net.createConnection(socketPath, () => { this.socket = sock; resolve(); });
      sock.on('data', (d: Buffer) => this.onData(d));
      sock.on('error', (e) => { if (!this.socket) reject(e); });
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

  onNotification(method: string, handler: (params: unknown) => void): void {
    const list = this.notifHandlers.get(method) ?? [];
    list.push(handler);
    this.notifHandlers.set(method, list);
  }

  onDisconnect(handler: () => void): void { this.closeHandlers.push(handler); }
  async disconnect(): Promise<void> { if (this.socket) { this.socket.end(); this.socket = null; } }
  get connected(): boolean { return this.socket !== null; }

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
    try { p = JSON.parse(json); } catch { return; }
    if ('id' in p && p.id != null) {
      const req = this.pending.get(p.id as number);
      if (req) {
        this.pending.delete(p.id as number);
        if ('error' in p) req.reject(new Error((p.error as { message: string }).message));
        else req.resolve(p.result);
      }
      return;
    }
    if ('method' in p) {
      const handlers = this.notifHandlers.get(p.method as string);
      if (handlers) for (const h of handlers) try { h(p.params); } catch {}
    }
  }

  private onClose() {
    for (const [, p] of this.pending) p.reject(new Error('Connection closed'));
    this.pending.clear();
    this.socket = null;
    for (const h of this.closeHandlers) try { h(); } catch {}
  }
}

// ═══════════════════════════════════════════════════════════════
// Daemon discovery
// ═══════════════════════════════════════════════════════════════
interface DaemonInfo { pid: number; cwd: string; session_id: string; socket: string; started_at: string; }

function getSocketDir(): string { return path.join(os.tmpdir(), 'baoclaw-sockets'); }

function discoverDaemons(): DaemonInfo[] {
  const dir = getSocketDir();
  if (!fs.existsSync(dir)) return [];
  const daemons: DaemonInfo[] = [];
  for (const file of fs.readdirSync(dir)) {
    if (!file.endsWith('.json')) continue;
    try {
      const meta: DaemonInfo = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf-8'));
      try { process.kill(meta.pid, 0); } catch { continue; }
      if (!fs.existsSync(meta.socket)) continue;
      daemons.push(meta);
    } catch {}
  }
  return daemons;
}

// ═══════════════════════════════════════════════════════════════
// Static file server
// ═══════════════════════════════════════════════════════════════
const MIME: Record<string, string> = {
  '.html': 'text/html', '.css': 'text/css', '.js': 'application/javascript',
  '.json': 'application/json', '.png': 'image/png', '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
};

function getPublicDir(): string {
  // Try multiple strategies to find the public directory
  // Strategy 1: relative to the script being executed (process.argv[1])
  const scriptPath = process.argv[1];
  if (scriptPath) {
    const candidate = path.join(path.dirname(scriptPath), '..', 'public');
    if (fs.existsSync(path.join(candidate, 'index.html'))) return path.resolve(candidate);
  }
  // Strategy 2: import.meta.url
  try {
    const thisFile = decodeURIComponent(new URL(import.meta.url).pathname);
    const candidate = path.join(path.dirname(thisFile), '..', 'public');
    if (fs.existsSync(path.join(candidate, 'index.html'))) return path.resolve(candidate);
  } catch {}
  // Strategy 3: __dirname equivalent via cwd
  const candidate = path.join(process.cwd(), 'public');
  if (fs.existsSync(path.join(candidate, 'index.html'))) return path.resolve(candidate);
  // Fallback
  console.error('Cannot find public directory!');
  return path.resolve('public');
}

const PUBLIC_DIR = getPublicDir();

function serveStatic(res: http.ServerResponse, urlPath: string) {
  const filePath = path.join(PUBLIC_DIR, urlPath === '/' ? 'index.html' : urlPath);
  const resolved = path.resolve(filePath);
  if (!resolved.startsWith(PUBLIC_DIR)) {
    res.writeHead(403); res.end('Forbidden'); return;
  }
  try {
    const data = fs.readFileSync(resolved);
    const ext = path.extname(resolved);
    res.writeHead(200, { 'Content-Type': MIME[ext] || 'application/octet-stream' });
    res.end(data);
  } catch {
    console.error(`404: ${resolved}`);
    res.writeHead(404); res.end('Not Found');
  }
}

// ═══════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════
async function main() {
  const args = process.argv.slice(2);
  const portIdx = args.indexOf('--port');
  const port = portIdx >= 0 && args[portIdx + 1] ? parseInt(args[portIdx + 1], 10) : 8080;
  const cwd = process.cwd();

  // Find daemon
  const daemons = discoverDaemons();
  if (daemons.length === 0) {
    console.error('No BaoClaw daemon found. Start one first with: baoclaw');
    process.exit(1);
  }
  const daemon = daemons[0];
  console.log(`Found daemon pid=${daemon.pid} cwd=${daemon.cwd}`);

  // HTTP server
  const server = http.createServer((req, res) => {
    if (req.method === 'GET') {
      serveStatic(res, req.url || '/');
    } else {
      res.writeHead(405); res.end();
    }
  });

  // WebSocket server
  const wss = new WebSocketServer({ noServer: true });

  server.on('upgrade', (req, socket, head) => {
    console.log(`HTTP upgrade request: ${req.url}`);
    wss.handleUpgrade(req, socket, head, (ws) => {
      wss.emit('connection', ws, req);
    });
  });

  wss.on('connection', async (ws: WebSocket, req: http.IncomingMessage) => {
    // Extract cwd from query parameter: ws://host/?cwd=/path/to/project
    const reqUrl = new URL(req.url || '/', `http://${req.headers.host}`);
    const wsCwd = reqUrl.searchParams.get('cwd') || cwd;
    console.log(`WebSocket client connected (cwd: ${wsCwd})`);

    // Each WS connection gets its own IPC client to the daemon
    const ipc = new IpcClient();
    try {
      console.log(`Connecting to daemon socket: ${daemon.socket}`);
      await ipc.connect(daemon.socket);
      console.log('IPC connected, sending initialize...');
      const initResult = await ipc.request('initialize', {
        cwd: wsCwd, settings: {}, shared_session_id: 'web',
      });
      console.log('Initialize done, sending to browser');
      // Send init info to browser
      ws.send(JSON.stringify({ type: 'init', data: initResult, cwd: wsCwd, daemon: { pid: daemon.pid } }));
    } catch (err: any) {
      console.error('IPC init failed:', err.message);
      ws.send(JSON.stringify({ type: 'error', message: `Failed to connect to daemon: ${err.message}` }));
      ws.close();
      return;
    }

    // Forward daemon stream events to browser
    ipc.onNotification('stream/event', (params) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'stream', data: params }));
      }
    });

    ipc.onDisconnect(() => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'error', message: 'Daemon disconnected' }));
        ws.close();
      }
    });

    // Handle messages from browser
    ws.on('message', async (raw: Buffer) => {
      let msg: { action: string; [k: string]: unknown };
      try { msg = JSON.parse(raw.toString()); } catch { return; }

      try {
        switch (msg.action) {
          case 'submit': {
            const result = await ipc.request('submitMessage', { prompt: msg.prompt });
            ws.send(JSON.stringify({ type: 'submitDone', data: result }));
            break;
          }
          case 'abort': {
            await ipc.request('abort');
            ws.send(JSON.stringify({ type: 'abortDone' }));
            break;
          }
          case 'compact': {
            const result = await ipc.request('compact');
            ws.send(JSON.stringify({ type: 'compactDone', data: result }));
            break;
          }
          case 'rpc': {
            // Generic RPC passthrough: { action: 'rpc', method: '...', params: {...} }
            const result = await ipc.request(msg.method as string, msg.params);
            ws.send(JSON.stringify({ type: 'rpcResult', method: msg.method, data: result }));
            break;
          }
          case 'permission': {
            await ipc.request('permissionResponse', {
              tool_use_id: msg.tool_use_id, decision: msg.decision, rule: msg.rule,
            });
            break;
          }
          default:
            ws.send(JSON.stringify({ type: 'error', message: `Unknown action: ${msg.action}` }));
        }
      } catch (err: any) {
        ws.send(JSON.stringify({ type: 'error', message: err.message }));
      }
    });

    ws.on('close', () => {
      console.log('WebSocket client disconnected');
      ipc.disconnect();
    });
  });

  server.listen(port, () => {
    console.log(`\n🐾 BaoClaw Web running at http://localhost:${port}`);
    console.log(`   CWD: ${cwd}`);
    console.log(`   Daemon: pid=${daemon.pid}`);
    console.log(`   Public: ${PUBLIC_DIR}\n`);
  });
}

main().catch((err) => { console.error('Fatal:', err); process.exit(1); });
