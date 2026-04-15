#!/usr/bin/env node
/**
 * BaoClaw CLI — Rich terminal interface powered by Rust core engine.
 * Visual style inspired by BaoClaw TUI.
 */
import * as net from 'net';
import * as readline from 'readline';
import * as path from 'path';
import { spawn, ChildProcess } from 'child_process';
import { renderMarkdown } from './markdownRenderer.js';
import * as fs from 'fs';
import * as os from 'os';
// @ts-ignore — pdf-parse and mammoth loaded dynamically for CJS compat
let pdf: any;
let mammoth: any;

// ═══════════════════════════════════════════════════════════════
// ANSI helpers
// ═══════════════════════════════════════════════════════════════
const ESC = '\x1b[';
const RESET = `${ESC}0m`;
const BOLD = `${ESC}1m`;
const DIM = `${ESC}2m`;
const ITALIC = `${ESC}3m`;
const UNDERLINE = `${ESC}4m`;

// Colors (optimized for dark terminal backgrounds)
const FG_ORANGE = `${ESC}38;2;217;119;40m`;   // BaoClaw orange
const FG_CYAN = `${ESC}96m`;                   // bright cyan
const FG_GREEN = `${ESC}92m`;                  // bright green
const FG_YELLOW = `${ESC}93m`;                 // bright yellow
const FG_RED = `${ESC}91m`;                    // bright red
const FG_MAGENTA = `${ESC}95m`;                // bright magenta
const FG_BLUE = `${ESC}94m`;                   // bright blue
const FG_WHITE = `${ESC}97m`;                  // bright white
const FG_GRAY = `${ESC}38;2;160;160;160m`;     // lighter gray (visible on dark bg)
const FG_BRIGHT_WHITE = `${ESC}97m`;
const BG_DARK = `${ESC}48;2;30;30;30m`;

// Clawd body color (warm tan/beige like the original)
const FG_CLAWD = `${ESC}38;2;210;180;140m`;
const BG_CLAWD = `${ESC}48;2;60;50;40m`;

// ═══════════════════════════════════════════════════════════════
// Spinner
// ═══════════════════════════════════════════════════════════════
const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
let spinnerInterval: ReturnType<typeof setInterval> | null = null;
let spinnerFrame = 0;
let spinnerMessage = '';

function startSpinner(msg: string) {
  spinnerMessage = msg;
  spinnerFrame = 0;
  if (spinnerInterval) clearInterval(spinnerInterval);
  spinnerInterval = setInterval(() => {
    const frame = SPINNER_FRAMES[spinnerFrame % SPINNER_FRAMES.length];
    process.stderr.write(`\r${FG_ORANGE}${frame}${RESET} ${DIM}${spinnerMessage}${RESET}  `);
    spinnerFrame++;
  }, 80);
}

function stopSpinner() {
  if (spinnerInterval) {
    clearInterval(spinnerInterval);
    spinnerInterval = null;
    process.stderr.write('\r' + ' '.repeat(60) + '\r');
  }
}

// ═══════════════════════════════════════════════════════════════
// ASCII Art Logo
// ═══════════════════════════════════════════════════════════════
function printLogo() {
  // White Bichon Frise dog — BaoClaw mascot (4 legs + tail)
  const W = `${ESC}38;2;255;255;255m`;  // white fur
  const B = `${ESC}38;2;40;40;40m`;     // black (eyes/nose)
  const P = `${ESC}38;2;255;182;193m`;  // pink (tongue)
  const S = `${ESC}38;2;220;220;220m`;  // light shadow
  const G = FG_GRAY;
  const O = FG_ORANGE;
  const R = RESET;

  const logo = `
${G}                                                                ${R}
${G}       ${W}░░${R}${G}         ${W}░░${R}${G}                                            ${R}
${G}       ${W}░░░${R}${G}       ${W}░░░${R}${G}                                            ${R}
${G}        ${W}░░░░░░░░░░░${R}${G}                                             ${R}
${G}      ${W}░░░░░░░░░░░░░░░${R}${G}                                           ${R}
${G}     ${W}░░░░░░░░░░░░░░░░░${R}${G}        ${O}╔╗   ╔╗${R}${G}                        ${R}
${G}    ${W}░░░░░${R}${B}██${R}${W}░░░░░${R}${B}██${R}${W}░░░░${R}${G}        ${O}║╚╗╔╝║${R}${G}                        ${R}
${G}    ${W}░░░░░░░░${R}${B}▄${R}${W}░░░░░░░░░${R}${G}        ${O}╚═╝╚═╝${R}${G}                        ${R}
${G}    ${W}░░░░░░░${R}${P}▀▀▀${R}${W}░░░░░░░░${R}${G}                                       ${R}
${G}     ${W}░░░░░░░░░░░░░░░░░${R}${G}    ${O}${BOLD}B a o C l a w${R}${G}                    ${R}
${G}    ${W}░░░░░░░░░░░░░░░░░░░░${R}${W}~${R}${G}                                      ${R}
${G}   ${W}░░░░░${R}${G}  ${W}░░░░░░░${R}${G}  ${W}░░░░░${R}${G}   ${S}AI Coding Assistant${R}${G}                ${R}
${G}   ${W}░░░░${R}${G}  ${W}░░░░${R}${G} ${W}░░░░${R}${G}  ${W}░░░░${R}${G}   ${S}Powered by Rust${R}${G}                  ${R}
${G}   ${W}░░░░${R}${G}  ${W}░░░░${R}${G} ${W}░░░░${R}${G}  ${W}░░░░${R}${G}                                    ${R}
${G}    ${W}░░${R}${G}    ${W}░░${R}${G}   ${W}░░${R}${G}    ${W}░░${R}${G}                                      ${R}
${G}                                                                ${R}
`;
  process.stdout.write(logo);
}

function printWelcome(sessionId: string, model: string, cwd: string) {
  const cols = process.stdout.columns || 80;
  const line = '─'.repeat(Math.min(cols - 2, 70));

  console.log(`${FG_ORANGE}${BOLD}  Welcome to BaoClaw ${RESET}${DIM}v0.9.0${RESET}`);
  console.log(`${FG_GRAY}${line}${RESET}`);
  console.log(`${DIM}  Session: ${sessionId}${RESET}`);
  console.log(`${DIM}  cwd: ${cwd}${RESET}`);
  console.log(`${DIM}  model: ${RESET}${FG_GREEN}${model}${RESET}`);
  console.log(`${FG_GRAY}${line}${RESET}`);
  console.log();
  console.log(`${DIM}  Tips: Type your message and press Enter.${RESET}`);
  console.log(`${DIM}        /tools    — list registered tools${RESET}`);
  console.log(`${DIM}        /mcp      — list MCP servers${RESET}`);
  console.log(`${DIM}        /skills   — list skills${RESET}`);
  console.log(`${DIM}        /plugins  — list plugins${RESET}`);
  console.log(`${DIM}        /compact  — compress conversation context${RESET}`);
  console.log(`${DIM}        /think    — toggle extended thinking${RESET}`);
  console.log(`${DIM}        /model    — show or switch model${RESET}`);
  console.log(`${DIM}        /help     — all commands${RESET}`);
  console.log(`${DIM}        /voice    — voice input (whisper.cpp)${RESET}`);
  console.log(`${DIM}        /quit     — disconnect (daemon stays running)${RESET}`);
  console.log(`${DIM}        /shutdown — stop daemon${RESET}`);
  console.log();
}

// ═══════════════════════════════════════════════════════════════
// Message formatting
// ═══════════════════════════════════════════════════════════════
function formatToolUse(toolName: string, input: unknown): string {
  const inputStr = typeof input === 'string' ? input : JSON.stringify(input, null, 2);
  const lines = inputStr.split('\n');
  const preview = lines.length > 8
    ? lines.slice(0, 8).join('\n') + `\n${DIM}... (${lines.length - 8} more lines)${RESET}`
    : inputStr;

  // Special formatting for common tools
  if (toolName === 'Bash') {
    const cmd = typeof input === 'object' && input !== null && 'command' in input
      ? (input as { command: string }).command
      : preview;
    return `${FG_MAGENTA}❯ ${BOLD}${toolName}${RESET}${FG_GRAY} $ ${RESET}${FG_WHITE}${cmd}${RESET}`;
  }
  if (toolName === 'FileRead' || toolName === 'Read') {
    const filePath = typeof input === 'object' && input !== null && 'file_path' in input
      ? (input as { file_path: string }).file_path
      : '';
    return `${FG_BLUE}📄 ${BOLD}${toolName}${RESET} ${FG_GRAY}${filePath}${RESET}`;
  }
  if (toolName === 'FileWrite' || toolName === 'Write') {
    const filePath = typeof input === 'object' && input !== null && 'file_path' in input
      ? (input as { file_path: string }).file_path
      : '';
    return `${FG_GREEN}✏️  ${BOLD}${toolName}${RESET} ${FG_GRAY}${filePath}${RESET}`;
  }
  if (toolName === 'FileEdit' || toolName === 'Edit') {
    const filePath = typeof input === 'object' && input !== null && 'file_path' in input
      ? (input as { file_path: string }).file_path
      : '';
    return `${FG_YELLOW}✎ ${BOLD}${toolName}${RESET} ${FG_GRAY}${filePath}${RESET}`;
  }

  return `${FG_MAGENTA}⚡ ${BOLD}${toolName}${RESET}\n${DIM}${preview}${RESET}`;
}

function formatToolResult(output: unknown, isError: boolean): string {
  const outputStr = typeof output === 'string'
    ? output
    : typeof output === 'object' && output !== null && 'output' in output
      ? String((output as { output: string }).output)
      : typeof output === 'object' && output !== null && 'stdout' in output
        ? String((output as { stdout: string }).stdout)
        : JSON.stringify(output);

  const maxLen = 800;
  const truncated = outputStr.length > maxLen
    ? outputStr.slice(0, maxLen) + `\n${DIM}... (truncated)${RESET}`
    : outputStr;

  const lines = truncated.split('\n');
  const color = isError ? FG_RED : FG_WHITE;
  const prefix = isError ? `${FG_RED}✗` : `${FG_GREEN}✓`;

  if (lines.length <= 1) {
    return `  ${prefix}${RESET} ${color}${truncated}${RESET}`;
  }

  return `  ${prefix}${RESET}\n${lines.map(l => `  ${color}${l}${RESET}`).join('\n')}`;
}

// ═══════════════════════════════════════════════════════════════
// Minimal IPC client (inline to avoid ESM import issues)
// ═══════════════════════════════════════════════════════════════
class IpcClient {
  private socket: net.Socket | null = null;
  private buffer = '';
  private nextId = 1;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private notifHandlers = new Map<string, ((params: unknown) => void)[]>();

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

  async disconnect(): Promise<void> {
    if (this.socket) { this.socket.end(); this.socket = null; }
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
  }
}

// ═══════════════════════════════════════════════════════════════
// Daemon discovery
// ═══════════════════════════════════════════════════════════════

interface DaemonInfo {
  pid: number;
  cwd: string;
  session_id: string;
  socket: string;
  started_at: string;
}

function getSocketDir(): string {
  return path.join(os.tmpdir(), 'baoclaw-sockets');
}

/** Scan for running BaoClaw daemon instances */
function discoverDaemons(): DaemonInfo[] {
  const dir = getSocketDir();
  if (!fs.existsSync(dir)) return [];

  const daemons: DaemonInfo[] = [];
  for (const file of fs.readdirSync(dir)) {
    if (!file.endsWith('.json')) continue;
    try {
      const meta: DaemonInfo = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf-8'));
      // Check if the process is still alive
      try { process.kill(meta.pid, 0); } catch { continue; } // dead process
      // Check if socket file exists
      if (!fs.existsSync(meta.socket)) continue;
      daemons.push(meta);
    } catch { /* skip invalid files */ }
  }
  return daemons;
}

/** Prompt user to select a daemon or start new */
async function selectDaemon(daemons: DaemonInfo[]): Promise<DaemonInfo | null> {
  return new Promise((resolve) => {
    console.log(`\n${FG_ORANGE}${BOLD}Running BaoClaw instances:${RESET}\n`);
    console.log(`  ${FG_WHITE}${BOLD}0${RESET}  ${FG_GREEN}Start new instance${RESET}`);
    for (let i = 0; i < daemons.length; i++) {
      const d = daemons[i];
      const age = timeSince(d.started_at);
      console.log(`  ${FG_WHITE}${BOLD}${i + 1}${RESET}  ${DIM}pid=${d.pid}${RESET} ${FG_WHITE}${d.cwd}${RESET} ${DIM}(${age}, session: ${d.session_id.slice(0, 8)}...)${RESET}`);
    }
    console.log();

    const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
    rl.question(`${FG_ORANGE}Select [0-${daemons.length}]:${RESET} `, (answer) => {
      rl.close();
      const idx = parseInt(answer.trim(), 10);
      if (isNaN(idx) || idx === 0 || idx > daemons.length) {
        resolve(null); // start new
      } else {
        resolve(daemons[idx - 1]);
      }
    });
  });
}

function timeSince(isoDate: string): string {
  const ms = Date.now() - new Date(isoDate).getTime();
  const mins = Math.floor(ms / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

// ═══════════════════════════════════════════════════════════════
// Daemon launcher
// ═══════════════════════════════════════════════════════════════
async function startNewDaemon(binaryPath: string): Promise<string> {
  startSpinner('Starting BaoClaw engine...');

  // Start as daemon: detached, with --daemon flag
  const child = spawn(binaryPath, ['--daemon', '--cwd', process.cwd()], {
    cwd: process.cwd(),
    stdio: ['ignore', 'pipe', 'pipe'],
    env: process.env,
    detached: true,  // Survives parent exit
  });

  // Don't let the child keep the parent alive
  child.unref();

  let stderr = '';
  child.stderr?.on('data', (d: Buffer) => { stderr += d.toString(); });

  const socketPath = await new Promise<string>((resolve, reject) => {
    let buf = '';
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`Timeout waiting for engine startup.\n${stderr}`));
    }, 60000);

    child.stdout?.on('data', (data: Buffer) => {
      buf += data.toString();
      for (const line of buf.split('\n')) {
        if (line.startsWith('SOCKET:')) {
          clearTimeout(timer);
          // Detach stdout after getting socket path
          child.stdout?.removeAllListeners();
          child.stderr?.removeAllListeners();
          resolve(line.slice('SOCKET:'.length).trim());
          return;
        }
      }
    });
    child.on('error', (e) => { clearTimeout(timer); reject(e); });
  });

  stopSpinner();
  return socketPath;
}

// ═══════════════════════════════════════════════════════════════
// Autocomplete
// ═══════════════════════════════════════════════════════════════
const COMMANDS = [
  '/tools', '/mcp', '/skills', '/plugins', '/help', '/quit',
  '/shutdown', '/compact', '/think', '/model', '/commit', '/diff', '/git',
  '/clear', '/abort', '/task', '/voice', '/telemetry', '/telegram', '/memory',
  '/cd', '/cron',
];

/**
 * Get file path completions for the given partial path.
 */
function getFileCompletions(partial: string): string[] {
  try {
    const dir = partial.includes('/')
      ? path.dirname(partial)
      : '.';
    const prefix = partial.includes('/')
      ? path.basename(partial)
      : partial;

    const dirPath = path.resolve(process.cwd(), dir);
    const entries = fs.readdirSync(dirPath, { withFileTypes: true });
    const matches: string[] = [];

    for (const entry of entries) {
      if (entry.name.startsWith(prefix)) {
        const full = dir === '.' ? entry.name : path.join(dir, entry.name);
        matches.push(entry.isDirectory() ? full + '/' : full);
      }
    }
    return matches;
  } catch {
    return [];
  }
}

/**
 * Readline completer: handles command and file path completion.
 */
function completer(line: string): [string[], string] {
  // Command completion
  if (line.startsWith('/')) {
    const matches = COMMANDS.filter(c => c.startsWith(line));
    return [matches, line];
  }

  // File path completion on the last whitespace-separated token
  const tokens = line.split(/\s+/);
  const last = tokens[tokens.length - 1] || '';

  // @file completion for attachments
  if (last.startsWith('@')) {
    const partial = last.slice(1);
    const matches = getFileCompletions(partial).map(m => '@' + m);
    return [matches.length > 0 ? matches : [last], last];
  }

  if (last.includes('/') || last.includes('.')) {
    const matches = getFileCompletions(last);
    return [matches.length > 0 ? matches : [last], last];
  }

  return [[], line];
}

// ═══════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════
async function main() {
  const defaultBin = path.resolve(process.cwd(), 'baoclaw-core', 'target', 'release', 'baoclaw-core');
  const binaryPath = path.resolve(process.env.BAOCLAW_CORE_BIN ?? defaultBin);

  // Parse CLI flags
  const args = process.argv.slice(2);
  let thinkingEnabled = false;
  let thinkingBudget = 10240;
  const vimMode = args.includes('--vim') || process.env.BAOCLAW_VIM === '1';
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--think') {
      thinkingEnabled = true;
      // Check if next arg is a number (budget)
      if (i + 1 < args.length && /^\d+$/.test(args[i + 1])) {
        thinkingBudget = parseInt(args[i + 1], 10);
        i++;
      }
    } else if (args[i]?.startsWith('--think=')) {
      thinkingEnabled = true;
      const val = args[i].split('=')[1];
      if (val && /^\d+$/.test(val)) {
        thinkingBudget = parseInt(val, 10);
      }
    }
  }

  // Check API key
  if (!process.env.ANTHROPIC_API_KEY) {
    console.error(`${FG_RED}${BOLD}Error:${RESET} ANTHROPIC_API_KEY is not set.`);
    console.error(`${DIM}Set it with: export ANTHROPIC_API_KEY=sk-ant-...${RESET}`);
    process.exit(1);
  }

  // Clear screen and print logo
  process.stdout.write(`${ESC}2J${ESC}H`);
  printLogo();

  // ── Discover existing daemons ──
  // Global daemon model: reuse any existing daemon, start new only if none exists.
  // Each CLI sends its own cwd; the daemon manages per-project sessions internally.
  const daemons = discoverDaemons();
  let socketPath: string;
  let child: ChildProcess | null = null;
  let isReconnect = false;
  const effectiveCwd = process.cwd(); // always use the terminal's current directory

  if (daemons.length > 0) {
    // Connect to the first available daemon (global singleton)
    const daemon = daemons[0];
    socketPath = daemon.socket;
    isReconnect = true;
    console.log(`${DIM}Connecting to daemon pid=${daemon.pid}...${RESET}`);
  } else {
    socketPath = await startNewDaemon(binaryPath);
  }

  // Connect IPC
  const client = new IpcClient();
  startSpinner('Connecting to engine (loading MCP servers)...');
  await client.connect(socketPath);

  // Initialize
  const thinkingSettings = thinkingEnabled
    ? { thinking: { mode: 'enabled', budget_tokens: thinkingBudget } }
    : {};
  const initResult = await client.request<{ capabilities: Record<string, unknown>; session_id: string; reconnected?: boolean; message_count?: number; shared?: boolean }>(
    'initialize',
    { cwd: effectiveCwd, settings: { ...thinkingSettings }, shared_session_id: 'default' }
  );

  stopSpinner();

  if (initResult.reconnected) {
    console.log(`\n${FG_GREEN}${BOLD}Reconnected${RESET} ${DIM}to session ${initResult.session_id} (${initResult.message_count} messages in history)${RESET}\n`);
  }
  const activeModel = process.env.ANTHROPIC_MODEL || (() => {
    try {
      const raw = fs.readFileSync(path.join(os.homedir(), '.baoclaw', 'config.json'), 'utf-8');
      return JSON.parse(raw).model || 'claude-sonnet-4-20250514';
    } catch { return 'claude-sonnet-4-20250514'; }
  })();
  printWelcome(initResult.session_id, activeModel, effectiveCwd);

  // ── Auto-register project and prompt for description if new ──
  try {
    const projCheck = await client.request<{ projects: any[] }>('projectsList');
    const existing = projCheck.projects.find((p: any) => p.cwd === effectiveCwd);
    if (!existing) {
      const defaultDesc = path.basename(effectiveCwd);
      const descRl = readline.createInterface({ input: process.stdin, output: process.stdout });
      const desc = await new Promise<string>((resolve) => {
        descRl.question(`${FG_ORANGE}Project description${RESET} ${DIM}[${defaultDesc}]${RESET}: `, (answer) => {
          descRl.close();
          resolve(answer.trim() || defaultDesc);
        });
      });
      await client.request('projectsNew', { cwd: effectiveCwd, description: desc });
      console.log(`${DIM}  Registered project: ${desc}${RESET}\n`);
    }
  } catch { /* ignore registration errors */ }

  // ── Stream event handling ──
  let isStreaming = false;
  let currentText = '';
  let toolCount = 0;
  let queryStartTime = 0;

  client.onNotification('stream/event', (params: unknown) => {
    const event = params as Record<string, unknown>;
    if (!event || typeof event !== 'object') return;

    switch (event.type) {
      case 'assistant_chunk': {
        stopSpinner();
        const content = (event as { content: string }).content;
        if (!isStreaming) {
          isStreaming = true;
        }
        currentText += content;
        break;
      }

      case 'thinking_chunk': {
        stopSpinner();
        const content = (event as { content: string }).content;
        if (!isStreaming) {
          process.stdout.write(`\n${FG_GRAY}${ITALIC}💭 Thinking...${RESET}\n`);
          isStreaming = true;
        }
        process.stdout.write(`${FG_GRAY}${content}${RESET}`);
        break;
      }

      case 'tool_use': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
        toolCount++;
        const tu = event as { tool_name: string; input: unknown; tool_use_id: string };
        console.log(`\n${formatToolUse(tu.tool_name, tu.input)}`);
        startSpinner(`Running ${tu.tool_name}...`);
        break;
      }

      case 'tool_result': {
        stopSpinner();
        const tr = event as { tool_use_id: string; output: unknown; is_error: boolean };
        console.log(formatToolResult(tr.output, tr.is_error));
        break;
      }

      case 'progress': {
        const pg = event as { tool_use_id: string; data: Record<string, unknown> };
        const info = pg.data?.sub_agent_tool || pg.data?.percent || pg.data?.message || '';
        if (spinnerInterval) {
          spinnerMessage = `${info}`;
        }
        break;
      }

      case 'permission_request': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
        const pr = event as { tool_name: string; input: Record<string, unknown>; tool_use_id: string };
        console.log(`\n${FG_YELLOW}⚠ ${BOLD}Permission Required${RESET}`);
        console.log(`  Tool: ${FG_WHITE}${pr.tool_name}${RESET}`);
        console.log(`  Input: ${DIM}${JSON.stringify(pr.input)}${RESET}`);
        console.log(`  ${FG_GREEN}[y]${RESET} Allow  ${FG_GREEN}[a]${RESET} Always Allow  ${FG_RED}[n]${RESET} Deny`);

        const permRl = readline.createInterface({ input: process.stdin, output: process.stdout });
        permRl.question(`${FG_ORANGE}> ${RESET}`, async (answer: string) => {
          permRl.close();
          let decision: string;
          let rule: string | undefined;
          switch (answer.trim().toLowerCase()) {
            case 'y': decision = 'allow'; break;
            case 'a': decision = 'allow_always'; rule = pr.tool_name; break;
            case 'n': default: decision = 'deny'; break;
          }
          try {
            await client.request('permissionResponse', {
              tool_use_id: pr.tool_use_id,
              decision,
              rule,
            });
          } catch (err) {
            console.error(`${FG_RED}Failed to send permission response: ${err}${RESET}`);
          }
          if (decision !== 'deny') {
            startSpinner(`Running ${pr.tool_name}...`);
          }
        });
        break;
      }

      case 'result': {
        stopSpinner();
        if (isStreaming) {
          // Render accumulated assistant text through Markdown renderer
          process.stdout.write(`\n${FG_ORANGE}${BOLD}BaoClaw${RESET}\n`);
          process.stdout.write(renderMarkdown(currentText));
          process.stdout.write('\n');
          isStreaming = false;
        }
        const result = event as { status: string; num_turns: number; duration_ms: number; usage?: { input_tokens: number; output_tokens: number } };
        const elapsed = Date.now() - queryStartTime;
        const tokens = result.usage
          ? `${result.usage.input_tokens}→${result.usage.output_tokens} tokens`
          : '';
        const tools = toolCount > 0 ? `${toolCount} tool${toolCount > 1 ? 's' : ''}` : '';
        const parts = [tools, tokens, `${(elapsed / 1000).toFixed(1)}s`].filter(Boolean).join(' · ');
        console.log(`\n${FG_CYAN}  ${parts}${RESET}\n`);
        queryStartTime = 0; // mark idle
        break;
      }

      case 'model_fallback': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
        const fb = event as { from_model: string; to_model: string };
        console.log(`\n${FG_YELLOW}⚠ ${BOLD}Model Fallback${RESET}${FG_YELLOW} ${fb.from_model} → ${fb.to_model} (rate limited)${RESET}\n`);
        startSpinner('Retrying with ' + fb.to_model + '...');
        break;
      }

      case 'error': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
        const err = event as { code: string; message: string };
        console.log(`\n${FG_RED}${BOLD}Error${RESET}${FG_RED} [${err.code}]: ${err.message}${RESET}\n`);
        queryStartTime = 0; // mark idle
        break;
      }
      case 'cron_result': {
        const cr = event as { job_id: string; job_name: string; text: string; timestamp: string };
        console.log(`\n${FG_CYAN}${BOLD}\u23F0 Cron: ${cr.job_name}${RESET} ${DIM}[${cr.job_id}]${RESET}`);
        const preview = cr.text.length > 500 ? cr.text.slice(0, 500) + '...' : cr.text;
        console.log(preview);
        console.log();
        rl.prompt();
        break;
      }
    }

  });

  // ── REPL ──
  if (vimMode) {
    // Node 22+ supports vi mode via this env var
    process.env.NODE_READLINE_VI_MODE = '1';
  }
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    prompt: `${FG_ORANGE}❯${RESET} `,
    completer,
    terminal: true,
  });

  // Ctrl+C handling: abort current task if busy, otherwise show hint
  let ctrlCCount = 0;
  rl.on('SIGINT', async () => {
    if (queryStartTime > 0) {
      // Task in progress — send abort
      stopSpinner();
      console.log(`\n${FG_YELLOW}⚠ Aborting...${RESET}`);
      try { await client.request('abort'); } catch {}
      ctrlCCount = 0;
    } else {
      ctrlCCount++;
      if (ctrlCCount >= 2) {
        console.log(`\n${DIM}Disconnected (daemon stays running).${RESET}`);
        await client.disconnect();
        process.exit(0);
      }
      console.log(`\n${DIM}Press Ctrl+C again to quit, or type /quit${RESET}`);
      rl.prompt();
      setTimeout(() => { ctrlCCount = 0; }, 2000);
    }
  });

  rl.prompt();

  // Paste detection: accumulate lines arriving within 50ms into a single input
  let pasteBuffer: string[] = [];
  let pasteTimer: ReturnType<typeof setTimeout> | null = null;
  let processingInput = false;

  async function handleInput(input: string) {
    if (processingInput) return;
    processingInput = true;
    try {
      await handleLine(input);
    } finally {
      processingInput = false;
    }
  }

  rl.on('line', (line: string) => {
    pasteBuffer.push(line);
    if (pasteTimer) clearTimeout(pasteTimer);
    pasteTimer = setTimeout(async () => {
      const lines = pasteBuffer;
      pasteBuffer = [];
      pasteTimer = null;

      // If single line, process normally
      if (lines.length === 1) {
        const input = lines[0].trim();
        if (!input) { rl.prompt(); return; }
        await handleInput(input);
        return;
      }

      // Multi-line paste: join with newlines
      const combined = lines.join('\n').trim();
      if (!combined) { rl.prompt(); return; }
      await handleInput(combined);
    }, 50);
  });

  async function handleLine(input: string) {

    if (input === '/quit' || input === '/exit' || input === '/q') {
      console.log(`\n${DIM}Disconnecting (daemon stays running)...${RESET}`);
      await client.disconnect();
      process.exit(0);
    }

    if (input === '/shutdown') {
      console.log(`\n${DIM}Shutting down daemon...${RESET}`);
      try { await client.request('shutdown'); } catch {}
      await client.disconnect();
      process.exit(0);
    }

    if (input === '/abort') {
      stopSpinner();
      try { await client.request('abort'); } catch {}
      console.log(`${FG_YELLOW}⚠ Aborted.${RESET}`);
      rl.prompt();
      return;
    }

    if (input === '/clear') {
      process.stdout.write(`${ESC}2J${ESC}H`);
      rl.prompt();
      return;
    }

    if (input === '/tools') {
      try {
        const result = await client.request<{ tools: Array<{ name: string; description: string; type: string }>; count: number }>('listTools');
        console.log(`\n${FG_ORANGE}${BOLD}Registered Tools${RESET} ${DIM}(${result.count})${RESET}\n`);
        for (const tool of result.tools) {
          const badge = tool.type === 'builtin' ? `${FG_GREEN}builtin${RESET}` : `${FG_BLUE}${tool.type}${RESET}`;
          console.log(`  ${FG_WHITE}${BOLD}${tool.name}${RESET} ${DIM}[${badge}${DIM}]${RESET}`);
          if (tool.description) {
            const desc = tool.description.length > 80 ? tool.description.slice(0, 80) + '...' : tool.description;
            console.log(`    ${DIM}${desc}${RESET}`);
          }
        }
        console.log();
      } catch (err) {
        console.error(`${FG_RED}Failed to list tools: ${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input === '/mcp') {
      try {
        const result = await client.request<{ servers: Array<{ name: string; command?: string; server_type: string; url?: string; disabled: boolean; source: string; config_path: string }>; count: number }>('listMcpServers');
        if (result.count === 0) {
          console.log(`\n${DIM}No MCP servers configured.${RESET}`);
          console.log(`${DIM}Add servers to .baoclaw/mcp.json or ~/.baoclaw/mcp.json${RESET}\n`);
        } else {
          console.log(`\n${FG_ORANGE}${BOLD}MCP Servers${RESET} ${DIM}(${result.count})${RESET}\n`);
          for (const srv of result.servers) {
            const status = srv.disabled ? `${FG_RED}disabled${RESET}` : `${FG_GREEN}enabled${RESET}`;
            const source = `${DIM}[${srv.source}]${RESET}`;
            console.log(`  ${FG_WHITE}${BOLD}${srv.name}${RESET} ${status} ${source}`);
            if (srv.command) {
              console.log(`    ${DIM}${srv.server_type}: ${srv.command} ${srv.args?.join(' ') || ''}${RESET}`);
            } else if (srv.url) {
              console.log(`    ${DIM}${srv.server_type}: ${srv.url}${RESET}`);
            }
            console.log(`    ${DIM}config: ${srv.config_path}${RESET}`);
          }
          console.log();
        }
      } catch (err) {
        console.error(`${FG_RED}Failed to list MCP servers: ${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input === '/skills') {
      try {
        const result = await client.request<{ skills: Array<{ name: string; path: string; source: string; description?: string }>; count: number }>('listSkills');
        if (result.count === 0) {
          console.log(`\n${DIM}No skills found.${RESET}`);
          console.log(`${DIM}Add skills to .baoclaw/skills/ or ~/.baoclaw/skills/${RESET}\n`);
        } else {
          console.log(`\n${FG_ORANGE}${BOLD}Skills${RESET} ${DIM}(${result.count})${RESET}\n`);
          for (const skill of result.skills) {
            const source = `${DIM}[${skill.source}]${RESET}`;
            console.log(`  ${FG_WHITE}${BOLD}${skill.name}${RESET} ${source}`);
            if (skill.description) {
              console.log(`    ${DIM}${skill.description}${RESET}`);
            }
            console.log(`    ${DIM}${skill.path}${RESET}`);
          }
          console.log();
        }
      } catch (err) {
        console.error(`${FG_RED}Failed to list skills: ${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input === '/plugins') {
      try {
        const result = await client.request<{ plugins: Array<{ name: string; version?: string; description?: string; path: string; source: string; has_tools: boolean; has_skills: boolean; has_mcp: boolean }>; count: number }>('listPlugins');
        if (result.count === 0) {
          console.log(`\n${DIM}No plugins found.${RESET}`);
          console.log(`${DIM}Add plugins to .baoclaw/plugins/ or ~/.baoclaw/plugins/${RESET}\n`);
        } else {
          console.log(`\n${FG_ORANGE}${BOLD}Plugins${RESET} ${DIM}(${result.count})${RESET}\n`);
          for (const plugin of result.plugins) {
            const ver = plugin.version ? ` ${DIM}v${plugin.version}${RESET}` : '';
            const source = `${DIM}[${plugin.source}]${RESET}`;
            const features: string[] = [];
            if (plugin.has_tools) features.push('tools');
            if (plugin.has_skills) features.push('skills');
            if (plugin.has_mcp) features.push('mcp');
            const featureStr = features.length > 0 ? ` ${DIM}(${features.join(', ')})${RESET}` : '';
            console.log(`  ${FG_WHITE}${BOLD}${plugin.name}${RESET}${ver} ${source}${featureStr}`);
            if (plugin.description) {
              console.log(`    ${DIM}${plugin.description}${RESET}`);
            }
          }
          console.log();
        }
      } catch (err) {
        console.error(`${FG_RED}Failed to list plugins: ${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input === '/model' || input.startsWith('/model ')) {
      const modelArg = input.slice('/model'.length).trim();
      if (!modelArg) {
        // Show current model, fallback chain, and config
        const configPath = path.join(os.homedir(), '.baoclaw', 'config.json');
        let fallbackModels: string[] = [];
        let maxRetries = 2;
        let configModel = 'claude-sonnet-4-20250514';
        try {
          const raw = fs.readFileSync(configPath, 'utf-8');
          const cfg = JSON.parse(raw);
          configModel = cfg.model || configModel;
          fallbackModels = cfg.fallback_models || [];
          maxRetries = cfg.max_retries_per_model ?? 2;
        } catch { /* use defaults */ }

        const activeModel = process.env.ANTHROPIC_MODEL || configModel;

        console.log(`\n${FG_ORANGE}${BOLD}Model Configuration${RESET}\n`);
        console.log(`  ${FG_WHITE}Active model:${RESET}  ${FG_GREEN}${activeModel}${RESET}`);
        if (process.env.ANTHROPIC_MODEL) {
          console.log(`  ${FG_GRAY}(overridden by ANTHROPIC_MODEL env var, config: ${configModel})${RESET}`);
        }
        console.log(`  ${FG_WHITE}Max retries:${RESET}   ${maxRetries} per model`);
        console.log();

        if (fallbackModels.length > 0) {
          console.log(`  ${FG_WHITE}Fallback chain:${RESET}`);
          console.log(`    ${FG_CYAN}0.${RESET} ${FG_GREEN}${activeModel}${RESET} ${FG_GRAY}(primary)${RESET}`);
          fallbackModels.forEach((m: string, i: number) => {
            console.log(`    ${FG_CYAN}${i + 1}.${RESET} ${FG_YELLOW}${m}${RESET}`);
          });
        } else {
          console.log(`  ${FG_GRAY}No fallback models configured.${RESET}`);
          console.log(`  ${FG_GRAY}Edit ~/.baoclaw/config.json to add fallback_models.${RESET}`);
        }

        console.log();
        console.log(`  ${FG_WHITE}Switch:${RESET}  /model <model-name>`);
        console.log(`  ${FG_WHITE}Config:${RESET}  ~/.baoclaw/config.json`);
        console.log();
      } else {
        // Switch model
        try {
          const result = await client.request<{ model: string }>('switchModel', { model: modelArg });
          console.log(`\n${FG_GREEN}${BOLD}Switched to ${result.model}${RESET}\n`);
        } catch (err) {
          console.error(`${FG_RED}Failed to switch model: ${err}${RESET}`);
        }
      }
      rl.prompt();
      return;
    }

    if (input === '/think') {
      thinkingEnabled = !thinkingEnabled;
      const settings = thinkingEnabled
        ? { thinking: { mode: 'enabled', budget_tokens: thinkingBudget } }
        : { thinking: { mode: 'disabled' } };
      try {
        await client.request('updateSettings', { settings });
        if (thinkingEnabled) {
          console.log(`\n${FG_GREEN}${BOLD}Extended thinking enabled${RESET} ${DIM}(budget: ${thinkingBudget} tokens)${RESET}\n`);
        } else {
          console.log(`\n${FG_YELLOW}Extended thinking disabled${RESET}\n`);
        }
      } catch (err) {
        console.error(`${FG_RED}Failed to update thinking settings: ${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input.startsWith('/cd')) {
      const targetDir = input.slice('/cd'.length).trim();
      if (!targetDir) {
        console.log(`\n${FG_WHITE}Current directory:${RESET} ${process.cwd()}`);
        console.log(`${DIM}Usage: /cd <path>${RESET}\n`);
        rl.prompt();
        return;
      }
      startSpinner('Switching directory...');
      try {
        const result = await client.request<{ cwd: string; scaffold_created: boolean; message_count?: number }>('switchCwd', { cwd: targetDir });
        stopSpinner();
        // Also change the Node process cwd for @file resolution
        try { process.chdir(result.cwd); } catch {}
        console.log(`\n${FG_GREEN}${BOLD}Switched to${RESET} ${result.cwd}`);
        if (result.scaffold_created) {
          console.log(`${DIM}  Created .baoclaw/ scaffold (BAOCLAW.md, mcp.json, skills/)${RESET}`);
        }
        if (result.message_count && result.message_count > 0) {
          console.log(`${DIM}  Resumed project session (${result.message_count} messages).${RESET}`);
        } else {
          console.log(`${DIM}  Fresh session started, project memory loaded.${RESET}`);
        }
        // Reset local streaming state
        currentText = '';
        isStreaming = false;
        toolCount = 0;
        console.log();
      } catch (err) {
        stopSpinner();
        console.error(`${FG_RED}${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input.startsWith('/cron')) {
      const cronArgs = input.slice('/cron'.length).trim();
      const parts = cronArgs.split(/\s+/);
      const subCmd = parts[0] || '';

      if (subCmd === 'add') {
        // /cron add "name" "every 1h" prompt text here
        const nameMatch = cronArgs.match(/add\s+"([^"]+)"\s+"([^"]+)"\s+(.+)/);
        if (!nameMatch) {
          console.log(`\n${FG_YELLOW}Usage: /cron add "job name" "every 1h" <prompt>${RESET}`);
          console.log(`${DIM}  Schedules: every 30m, every 2h, daily 09:00, weekly mon 09:00${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const result = await client.request<{ job: any }>('cronAdd', {
            name: nameMatch[1], schedule: nameMatch[2], prompt: nameMatch[3],
          });
          console.log(`\n${FG_GREEN}\u2713 Cron job created${RESET} ${DIM}[${result.job.id}] ${result.job.name} (${result.job.schedule})${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else if (subCmd === 'list' || subCmd === '') {
        try {
          const result = await client.request<{ jobs: any[]; count: number }>('cronList');
          if (result.count === 0) {
            console.log(`\n${DIM}No cron jobs. Use /cron add to create one.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Cron Jobs${RESET} ${DIM}(${result.count})${RESET}\n`);
            for (const j of result.jobs) {
              const status = j.enabled ? `${FG_GREEN}on${RESET}` : `${FG_RED}off${RESET}`;
              const last = j.last_run ? `last: ${j.last_run.slice(0, 19)}` : 'never run';
              console.log(`  ${FG_WHITE}${BOLD}${j.id}${RESET} ${status} ${DIM}${j.schedule}${RESET} ${j.name}`);
              console.log(`    ${DIM}${last}${RESET}`);
              console.log(`    ${DIM}${j.prompt.slice(0, 80)}${RESET}`);
            }
            console.log();
          }
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else if (subCmd === 'remove' || subCmd === 'rm') {
        const jobId = parts[1];
        if (!jobId) { console.log(`${FG_YELLOW}Usage: /cron remove <id>${RESET}`); rl.prompt(); return; }
        try {
          const result = await client.request<{ removed: boolean }>('cronRemove', { id: jobId });
          console.log(result.removed ? `\n${FG_GREEN}\u2713 Removed${RESET}\n` : `\n${FG_YELLOW}Not found${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else if (subCmd === 'toggle') {
        const jobId = parts[1];
        if (!jobId) { console.log(`${FG_YELLOW}Usage: /cron toggle <id>${RESET}`); rl.prompt(); return; }
        try {
          const result = await client.request<{ enabled: boolean }>('cronToggle', { id: jobId });
          console.log(`\n${result.enabled ? FG_GREEN + 'Enabled' : FG_YELLOW + 'Disabled'}${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else {
        console.log(`\n${FG_ORANGE}${BOLD}Cron Commands${RESET}\n`);
        console.log(`  ${FG_WHITE}/cron list${RESET}                              ${DIM}List all jobs${RESET}`);
        console.log(`  ${FG_WHITE}/cron add "name" "schedule" prompt${RESET}     ${DIM}Create a job${RESET}`);
        console.log(`  ${FG_WHITE}/cron remove <id>${RESET}                      ${DIM}Delete a job${RESET}`);
        console.log(`  ${FG_WHITE}/cron toggle <id>${RESET}                      ${DIM}Enable/disable${RESET}`);
        console.log(`\n${DIM}  Schedules: every 30m, every 2h, daily 09:00, weekly mon 09:00${RESET}\n`);
      }
      rl.prompt();
      return;
    }

    if (input === '/compact') {
      startSpinner('Compacting conversation...');
      try {
        const result = await client.request<{ tokens_saved: number; summary_tokens: number; tokens_before: number; tokens_after: number }>('compact');
        stopSpinner();
        if (result.tokens_saved === 0) {
          console.log(`\n${DIM}Not enough messages to compact.${RESET}\n`);
        } else {
          console.log(`\n${FG_GREEN}${BOLD}Compacted${RESET} ${DIM}${result.tokens_before}→${result.tokens_after} tokens (saved ${result.tokens_saved}, summary ${result.summary_tokens})${RESET}\n`);
        }
      } catch (err) {
        stopSpinner();
        console.error(`${FG_RED}Failed to compact: ${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input.startsWith('/memory')) {
      const memArgs = input.slice('/memory'.length).trim();
      const subCmd = memArgs.split(/\s+/)[0] || '';
      const rest = memArgs.slice(subCmd.length).trim();

      if (subCmd === 'list' || subCmd === 'ls') {
        try {
          const result = await client.request<{ memories: any[]; count: number }>('memoryList');
          if (result.count === 0) {
            console.log(`\n${DIM}No memories stored.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Long-term Memory${RESET} ${DIM}(${result.count})${RESET}\n`);
            for (const m of result.memories) {
              console.log(`  ${FG_WHITE}${BOLD}${m.id}${RESET} ${DIM}[${m.category}]${RESET} ${m.content}`);
            }
            console.log();
          }
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else if (subCmd === 'add') {
        // /memory add [category] content
        const parts = rest.split(/\s+/);
        let category = 'fact';
        let content = rest;
        if (parts[0] && ['fact', 'preference', 'pref', 'decision', 'dec'].includes(parts[0])) {
          category = parts[0];
          content = parts.slice(1).join(' ');
        }
        if (!content) {
          console.log(`\n${FG_YELLOW}Usage: /memory add [fact|preference|decision] <content>${RESET}\n`);
        } else {
          try {
            const result = await client.request<{ memory: any }>('memoryAdd', { content, category });
            console.log(`\n${FG_GREEN}✓ Memory added${RESET} ${DIM}[${result.memory.id}] ${result.memory.content}${RESET}\n`);
          } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        }
      } else if (subCmd === 'delete' || subCmd === 'del' || subCmd === 'rm') {
        if (!rest) {
          console.log(`\n${FG_YELLOW}Usage: /memory delete <id>${RESET}\n`);
        } else {
          try {
            const result = await client.request<{ deleted: boolean }>('memoryDelete', { id: rest });
            if (result.deleted) {
              console.log(`\n${FG_GREEN}✓ Memory deleted${RESET}\n`);
            } else {
              console.log(`\n${FG_YELLOW}Memory not found: ${rest}${RESET}\n`);
            }
          } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        }
      } else if (subCmd === 'clear') {
        try {
          const result = await client.request<{ cleared: number }>('memoryClear');
          console.log(`\n${FG_GREEN}✓ Cleared ${result.cleared} memories${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else {
        console.log(`\n${FG_ORANGE}${BOLD}Memory Commands${RESET}\n`);
        console.log(`  ${FG_WHITE}/memory list${RESET}                    ${DIM}List all memories${RESET}`);
        console.log(`  ${FG_WHITE}/memory add [category] <text>${RESET}  ${DIM}Add a memory (fact/preference/decision)${RESET}`);
        console.log(`  ${FG_WHITE}/memory delete <id>${RESET}            ${DIM}Delete a memory${RESET}`);
        console.log(`  ${FG_WHITE}/memory clear${RESET}                  ${DIM}Clear all memories${RESET}`);
        console.log();
      }
      rl.prompt();
      return;
    }

    if (input === '/diff') {
      startSpinner('Running git diff...');
      try {
        const result = await client.request<{ diff: string }>('gitDiff');
        stopSpinner();
        console.log(`\n${FG_ORANGE}${BOLD}Git Diff${RESET}\n`);
        console.log(result.diff);
        console.log();
      } catch (err) {
        stopSpinner();
        console.error(`${FG_RED}${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input.startsWith('/commit')) {
      const message = input.slice('/commit'.length).trim();
      if (!message) {
        console.log(`\n${FG_YELLOW}Usage: /commit <message>${RESET}\n`);
        rl.prompt();
        return;
      }
      startSpinner('Committing...');
      try {
        const result = await client.request<{ hash: string; message: string }>('gitCommit', { message });
        stopSpinner();
        console.log(`\n${FG_GREEN}${BOLD}Committed${RESET} ${DIM}${result.hash}${RESET} ${result.message}\n`);
      } catch (err) {
        stopSpinner();
        console.error(`${FG_RED}${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input === '/git') {
      startSpinner('Getting git status...');
      try {
        const result = await client.request<{
          branch: string | null;
          has_changes: boolean;
          staged_files: string[];
          modified_files: string[];
          untracked_files: string[];
        }>('gitStatus');
        stopSpinner();
        console.log(`\n${FG_ORANGE}${BOLD}Git Status${RESET}\n`);
        if (result.branch) {
          console.log(`  ${FG_WHITE}Branch:${RESET} ${result.branch}`);
        }
        if (!result.has_changes) {
          console.log(`  ${DIM}No changes${RESET}`);
        } else {
          if (result.staged_files.length > 0) {
            console.log(`  ${FG_GREEN}Staged:${RESET}`);
            for (const f of result.staged_files) {
              console.log(`    ${FG_GREEN}+${RESET} ${f}`);
            }
          }
          if (result.modified_files.length > 0) {
            console.log(`  ${FG_YELLOW}Modified:${RESET}`);
            for (const f of result.modified_files) {
              console.log(`    ${FG_YELLOW}~${RESET} ${f}`);
            }
          }
          if (result.untracked_files.length > 0) {
            console.log(`  ${DIM}Untracked:${RESET}`);
            for (const f of result.untracked_files) {
              console.log(`    ${DIM}?${RESET} ${f}`);
            }
          }
        }
        console.log();
      } catch (err) {
        stopSpinner();
        console.error(`${FG_RED}${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    // ── /task commands ──
    if (input.startsWith('/task')) {
      const taskArgs = input.slice('/task'.length).trim();
      const parts = taskArgs.split(/\s+/);
      const subCmd = parts[0] || '';

      if (subCmd === 'run') {
        const desc = taskArgs.slice('run'.length).trim().replace(/^["']|["']$/g, '');
        if (!desc) {
          console.log(`\n${FG_YELLOW}Usage: /task run "description"${RESET}\n`);
          rl.prompt();
          return;
        }
        startSpinner('Creating background task...');
        try {
          const result = await client.request<{ task_id: string }>('taskCreate', {
            description: desc,
            prompt: desc,
          });
          stopSpinner();
          console.log(`\n${FG_GREEN}${BOLD}Task created${RESET} ${DIM}id=${result.task_id}${RESET}\n`);
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}Failed to create task: ${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'list' || subCmd === '') {
        try {
          const result = await client.request<{ tasks: Array<{ id: string; description: string; status: string | { Failed: string }; created_at: string; completed_at: string | null; result: string | null }>; count: number }>('taskList');
          if (result.count === 0) {
            console.log(`\n${DIM}No background tasks.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Background Tasks${RESET} ${DIM}(${result.count})${RESET}\n`);
            for (const t of result.tasks) {
              const statusStr = typeof t.status === 'string' ? t.status
                : t.status && typeof t.status === 'object' && 'Failed' in t.status ? `Failed: ${t.status.Failed}` : JSON.stringify(t.status);
              const statusColor = statusStr === 'Running' ? FG_YELLOW
                : statusStr === 'Completed' ? FG_GREEN
                : statusStr === 'Aborted' ? FG_GRAY
                : FG_RED;
              console.log(`  ${FG_WHITE}${BOLD}${t.id}${RESET}  ${statusColor}${statusStr}${RESET}  ${DIM}${t.description}${RESET}`);
            }
            console.log();
          }
        } catch (err) {
          console.error(`${FG_RED}Failed to list tasks: ${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'status') {
        const taskId = parts[1] || '';
        if (!taskId) {
          console.log(`\n${FG_YELLOW}Usage: /task status <id>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const t = await client.request<{ id: string; description: string; status: string | { Failed: string }; created_at: string; completed_at: string | null; result: string | null }>('taskStatus', { task_id: taskId });
          const statusStr = typeof t.status === 'string' ? t.status
            : t.status && typeof t.status === 'object' && 'Failed' in t.status ? `Failed: ${t.status.Failed}` : JSON.stringify(t.status);
          console.log(`\n${FG_ORANGE}${BOLD}Task ${t.id}${RESET}`);
          console.log(`  Status:      ${statusStr}`);
          console.log(`  Description: ${t.description}`);
          console.log(`  Created:     ${t.created_at}`);
          if (t.completed_at) console.log(`  Completed:   ${t.completed_at}`);
          if (t.result) {
            const preview = t.result.length > 200 ? t.result.slice(0, 200) + '...' : t.result;
            console.log(`  Result:      ${DIM}${preview}${RESET}`);
          }
          console.log();
        } catch (err) {
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'stop') {
        const taskId = parts[1] || '';
        if (!taskId) {
          console.log(`\n${FG_YELLOW}Usage: /task stop <id>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const result = await client.request<{ stopped: boolean }>('taskStop', { task_id: taskId });
          if (result.stopped) {
            console.log(`\n${FG_GREEN}Task ${taskId} stopped.${RESET}\n`);
          } else {
            console.log(`\n${FG_YELLOW}Task ${taskId} was not running or not found.${RESET}\n`);
          }
        } catch (err) {
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      // Unknown /task subcommand
      console.log(`\n${FG_YELLOW}Usage: /task run "desc" | /task list | /task status <id> | /task stop <id>${RESET}\n`);
      rl.prompt();
      return;
    }

    if (input === '/voice') {
      // Voice input: record audio via arecord, transcribe via whisper-cli
      const whisperBin = process.env.WHISPER_CLI || 'whisper-cli';
      const whisperModel = process.env.WHISPER_MODEL || path.join(os.homedir(), '.baoclaw', 'models', 'ggml-base.bin');

      // Check if whisper-cli is available
      try {
        require('child_process').execSync(`which ${whisperBin}`, { stdio: 'ignore' });
      } catch {
        console.log(`\n${FG_YELLOW}whisper-cli not found.${RESET}`);
        console.log(`${DIM}Install whisper.cpp and ensure 'whisper-cli' is in PATH.${RESET}`);
        console.log(`${DIM}Or set WHISPER_CLI env var to the binary path.${RESET}`);
        console.log(`${DIM}Model path: ${whisperModel}${RESET}`);
        console.log(`${DIM}  Set WHISPER_MODEL env var to override.${RESET}\n`);
        rl.prompt();
        return;
      }

      // Check if model file exists
      if (!fs.existsSync(whisperModel)) {
        console.log(`\n${FG_YELLOW}Whisper model not found at: ${whisperModel}${RESET}`);
        console.log(`${DIM}Download a model:${RESET}`);
        console.log(`${DIM}  mkdir -p ~/.baoclaw/models${RESET}`);
        console.log(`${DIM}  curl -L -o ~/.baoclaw/models/ggml-base.bin \\${RESET}`);
        console.log(`${DIM}    https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin${RESET}`);
        console.log(`${DIM}Or set WHISPER_MODEL env var to your model path.${RESET}\n`);
        rl.prompt();
        return;
      }

      const tmpWav = path.join(os.tmpdir(), `baoclaw-voice-${Date.now()}.wav`);

      console.log(`\n${FG_ORANGE}${BOLD}🎤 Recording...${RESET} ${DIM}Press Enter to stop.${RESET}`);

      // Start recording with arecord (Linux) or sox (cross-platform fallback)
      let recProc: ChildProcess;
      try {
        // Try arecord first (ALSA, common on Linux)
        recProc = spawn('arecord', ['-f', 'S16_LE', '-r', '16000', '-c', '1', '-t', 'wav', tmpWav], {
          stdio: ['pipe', 'ignore', 'ignore'],
        });
      } catch {
        try {
          // Fallback to sox/rec
          recProc = spawn('rec', ['-r', '16000', '-c', '1', '-b', '16', tmpWav], {
            stdio: ['pipe', 'ignore', 'ignore'],
          });
        } catch {
          console.log(`${FG_RED}No audio recorder found. Install arecord (alsa-utils) or sox.${RESET}\n`);
          rl.prompt();
          return;
        }
      }

      // Wait for Enter to stop recording
      await new Promise<void>((resolve) => {
        const stopRl = readline.createInterface({ input: process.stdin, output: process.stdout });
        stopRl.once('line', () => {
          stopRl.close();
          recProc.kill('SIGTERM');
          resolve();
        });
      });

      // Wait for process to exit
      await new Promise<void>((resolve) => {
        recProc.on('close', () => resolve());
        setTimeout(() => { recProc.kill('SIGKILL'); resolve(); }, 2000);
      });

      if (!fs.existsSync(tmpWav) || fs.statSync(tmpWav).size < 100) {
        console.log(`${FG_YELLOW}Recording too short or failed.${RESET}\n`);
        try { fs.unlinkSync(tmpWav); } catch {}
        rl.prompt();
        return;
      }

      // Transcribe with whisper-cli
      startSpinner('Transcribing...');
      try {
        const result = require('child_process').execSync(
          `${whisperBin} -m "${whisperModel}" -f "${tmpWav}" -l auto --no-timestamps -otxt 2>/dev/null`,
          { encoding: 'utf-8', timeout: 30000 }
        ).trim();

        stopSpinner();

        // Also check for .txt output file (whisper-cli sometimes writes to file)
        let transcript = result;
        const txtFile = tmpWav + '.txt';
        if ((!transcript || transcript.length < 2) && fs.existsSync(txtFile)) {
          transcript = fs.readFileSync(txtFile, 'utf-8').trim();
          try { fs.unlinkSync(txtFile); } catch {}
        }

        if (!transcript || transcript.length < 2) {
          console.log(`${FG_YELLOW}Could not transcribe audio.${RESET}\n`);
        } else {
          console.log(`${FG_GREEN}📝 ${transcript}${RESET}\n`);

          // Submit the transcribed text as a message
          console.log(`${FG_BRIGHT_WHITE}${BOLD}You${RESET} ${transcript}`);
          currentText = '';
          isStreaming = false;
          toolCount = 0;
          queryStartTime = Date.now();
          startSpinner('Thinking...');
          try {
            await client.request('submitMessage', { prompt: transcript });
          } catch (err) {
            stopSpinner();
            console.error(`${FG_RED}Request failed: ${err}${RESET}`);
          }
        }
      } catch (err) {
        stopSpinner();
        console.error(`${FG_RED}Transcription failed: ${err}${RESET}`);
      }

      // Cleanup
      try { fs.unlinkSync(tmpWav); } catch {}

      rl.prompt();
      return;
    }

    if (input.startsWith('/telegram')) {
      const telegramArgs = input.slice('/telegram'.length).trim();
      const subCmd = telegramArgs.split(/\s+/)[0] || '';
      const baoclawHome = process.env.BAOCLAW_HOME || path.join(os.homedir(), '.baoclaw');
      const tgPidFile = path.join(os.homedir(), '.baoclaw', 'telegram-gateway.pid');
      const tgLogFile = path.join(os.homedir(), '.baoclaw', 'telegram-gateway.log');
      const gatewayScript = path.join(baoclawHome, 'baoclaw-telegram', 'src', 'gateway.ts');

      if (subCmd === 'start') {
        // Check if already running
        if (fs.existsSync(tgPidFile)) {
          try {
            const pidData = JSON.parse(fs.readFileSync(tgPidFile, 'utf-8'));
            try {
              process.kill(pidData.pid, 0);
              console.log(`\n${FG_YELLOW}Telegram gateway already running (pid=${pidData.pid}, @${pidData.bot_username || '?'}).${RESET}\n`);
              rl.prompt();
              return;
            } catch { /* dead process, continue */ }
          } catch { /* invalid pid file, continue */ }
        }

        // Find tsx binary from the telegram gateway's node_modules
        const tsxBin = path.join(baoclawHome, 'baoclaw-telegram', 'node_modules', '.bin', 'tsx');
        const tsxPath = fs.existsSync(tsxBin) ? tsxBin : path.join(path.dirname(process.execPath), 'tsx');

        // Spawn gateway as detached background process
        const logFd = fs.openSync(tgLogFile, 'a');
        try {
          const child = spawn(process.execPath, [tsxPath, gatewayScript], {
            cwd: process.cwd(),
            stdio: ['ignore', logFd, logFd],
            env: { ...process.env, BAOCLAW_TELEGRAM_CWD: process.cwd() },
            detached: true,
          });
          child.on('error', (err) => {
            console.error(`${FG_RED}Failed to spawn gateway: ${err.message}${RESET}`);
          });
          child.unref();
          console.log(`\n${FG_GREEN}${BOLD}Telegram gateway starting...${RESET}`);
          console.log(`${DIM}  Log: ${tgLogFile}${RESET}`);
          console.log(`${DIM}  PID file: ${tgPidFile}${RESET}\n`);
        } finally {
          fs.closeSync(logFd);
        }
      } else if (subCmd === 'stop') {
        if (!fs.existsSync(tgPidFile)) {
          console.log(`\n${FG_YELLOW}Telegram gateway is not running (no PID file).${RESET}\n`);
        } else {
          try {
            const pidData = JSON.parse(fs.readFileSync(tgPidFile, 'utf-8'));
            try {
              process.kill(pidData.pid, 'SIGTERM');
              console.log(`\n${FG_GREEN}Sent SIGTERM to Telegram gateway (pid=${pidData.pid}).${RESET}\n`);
            } catch {
              console.log(`\n${FG_YELLOW}Process ${pidData.pid} not found. Cleaning up PID file.${RESET}\n`);
              try { fs.unlinkSync(tgPidFile); } catch {}
            }
          } catch {
            console.log(`\n${FG_RED}Invalid PID file.${RESET}\n`);
          }
        }
      } else if (subCmd === 'status') {
        if (!fs.existsSync(tgPidFile)) {
          console.log(`\n${FG_YELLOW}Telegram gateway is not running.${RESET}\n`);
        } else {
          try {
            const pidData = JSON.parse(fs.readFileSync(tgPidFile, 'utf-8'));
            let alive = false;
            try { process.kill(pidData.pid, 0); alive = true; } catch {}
            if (alive) {
              console.log(`\n${FG_GREEN}${BOLD}Telegram gateway is running${RESET}`);
              console.log(`  ${FG_WHITE}PID:${RESET}      ${pidData.pid}`);
              if (pidData.bot_username) console.log(`  ${FG_WHITE}Bot:${RESET}      @${pidData.bot_username}`);
              if (pidData.daemon_pid) console.log(`  ${FG_WHITE}Daemon:${RESET}   pid=${pidData.daemon_pid}`);
              if (pidData.started_at) console.log(`  ${FG_WHITE}Started:${RESET}  ${pidData.started_at}`);
              console.log();
            } else {
              console.log(`\n${FG_YELLOW}Telegram gateway is not running (stale PID file).${RESET}\n`);
              try { fs.unlinkSync(tgPidFile); } catch {}
            }
          } catch {
            console.log(`\n${FG_RED}Invalid PID file.${RESET}\n`);
          }
        }
      } else {
        console.log(`\n${FG_ORANGE}${BOLD}Telegram Gateway${RESET}\n`);
        console.log(`  ${FG_WHITE}/telegram start${RESET}   ${DIM}Start the Telegram gateway${RESET}`);
        console.log(`  ${FG_WHITE}/telegram stop${RESET}    ${DIM}Stop the Telegram gateway${RESET}`);
        console.log(`  ${FG_WHITE}/telegram status${RESET}  ${DIM}Check gateway status${RESET}`);
        console.log();
        console.log(`  ${DIM}Config in ~/.baoclaw/config.json:${RESET}`);
        console.log(`  ${DIM}{${RESET}`);
        console.log(`  ${DIM}  "telegram": {${RESET}`);
        console.log(`  ${DIM}    "token": "123456:ABC-DEF...",${RESET}`);
        console.log(`  ${DIM}    "allowedChatIds": [12345678]${RESET}`);
        console.log(`  ${DIM}  }${RESET}`);
        console.log(`  ${DIM}}${RESET}`);
        console.log();
        console.log(`  ${DIM}Or set TELEGRAM_BOT_TOKEN env var.${RESET}`);
        console.log();
      }
      rl.prompt();
      return;
    }

    if (input.startsWith('/telemetry')) {
      const arg = input.slice('/telemetry'.length).trim().toLowerCase();
      if (arg === 'on') {
        console.log(`\n${FG_GREEN}${BOLD}Telemetry enabled${RESET} ${DIM}(events stored locally in ~/.baoclaw/telemetry/)${RESET}\n`);
      } else if (arg === 'off') {
        console.log(`\n${FG_YELLOW}Telemetry disabled${RESET}\n`);
      } else {
        console.log(`\n${FG_YELLOW}Usage: /telemetry on|off${RESET}\n`);
      }
      rl.prompt();
      return;
    }

    if (input === '/help') {
      console.log(`\n${FG_ORANGE}${BOLD}Commands${RESET}\n`);
      console.log(`  ${FG_WHITE}/tools${RESET}      ${DIM}List registered tools${RESET}`);
      console.log(`  ${FG_WHITE}/mcp${RESET}        ${DIM}List MCP server configurations${RESET}`);
      console.log(`  ${FG_WHITE}/skills${RESET}     ${DIM}List discovered skills${RESET}`);
      console.log(`  ${FG_WHITE}/plugins${RESET}    ${DIM}List discovered plugins${RESET}`);
      console.log(`  ${FG_WHITE}/compact${RESET}    ${DIM}Compress conversation context${RESET}`);
      console.log(`  ${FG_WHITE}/cd${RESET}         ${DIM}Switch working directory: /cd <path>${RESET}`);
      console.log(`  ${FG_WHITE}/think${RESET}      ${DIM}Toggle extended thinking mode${RESET}`);
      console.log(`  ${FG_WHITE}/model${RESET}      ${DIM}Show or switch model: /model [name]${RESET}`);
      console.log(`  ${FG_WHITE}/diff${RESET}       ${DIM}Show git diff summary${RESET}`);
      console.log(`  ${FG_WHITE}/commit${RESET}     ${DIM}Stage all and commit: /commit <message>${RESET}`);
      console.log(`  ${FG_WHITE}/git${RESET}        ${DIM}Show git status (branch, changes)${RESET}`);
      console.log(`  ${FG_WHITE}/task${RESET}       ${DIM}Background tasks: run, list, status, stop${RESET}`);
      console.log(`  ${FG_WHITE}/voice${RESET}      ${DIM}Voice input (requires whisper.cpp)${RESET}`);
      console.log(`  ${FG_WHITE}@file.pdf${RESET}   ${DIM}Attach file: @photo.png @doc.pdf @doc.docx${RESET}`);
      console.log(`  ${FG_WHITE}/telemetry${RESET}  ${DIM}Toggle telemetry: /telemetry on|off${RESET}`);
      console.log(`  ${FG_WHITE}/telegram${RESET}   ${DIM}Manage Telegram gateway: start, stop, status${RESET}`);
      console.log(`  ${FG_WHITE}/memory${RESET}     ${DIM}Long-term memory: list, add, delete, clear${RESET}`);
      console.log(`  ${FG_WHITE}/cron${RESET}       ${DIM}Scheduled tasks: add, list, remove, toggle${RESET}`);
      console.log(`  ${FG_WHITE}/abort${RESET}      ${DIM}Cancel current request${RESET}`);
      console.log(`  ${FG_WHITE}/clear${RESET}      ${DIM}Clear screen${RESET}`);
      console.log(`  ${FG_WHITE}/help${RESET}       ${DIM}Show this help${RESET}`);
      console.log(`  ${FG_WHITE}/quit${RESET}       ${DIM}Disconnect (daemon keeps running)${RESET}`);
      console.log(`  ${FG_WHITE}/shutdown${RESET}   ${DIM}Stop the daemon process${RESET}`);
      console.log();
      rl.prompt();
      return;
    }

    // Display user message
    console.log(`\n${FG_BRIGHT_WHITE}${BOLD}You${RESET} ${input}`);

    // Reset state
    currentText = '';
    isStreaming = false;
    toolCount = 0;
    queryStartTime = Date.now();

    startSpinner('Thinking...');

    // Check for @file references and convert to attachments
    let submitPayload: Record<string, unknown> = { prompt: input };
    const atFileRegex = /@(\S+\.(png|jpg|jpeg|gif|webp|pdf|docx|doc))/gi;
    const atMatches = input.match(atFileRegex);
    if (atMatches) {
      const attachments: Array<Record<string, unknown>> = [];
      let textPart = input;
      for (const match of atMatches) {
        const filePath = match.slice(1); // remove @
        const absPath = path.resolve(process.cwd(), filePath);
        const ext = path.extname(filePath).toLowerCase().slice(1);
        try {
          const fileData = fs.readFileSync(absPath);
          if (['png', 'jpg', 'jpeg', 'gif', 'webp'].includes(ext)) {
            // Image attachment
            const mediaType = ext === 'jpg' ? 'image/jpeg' : `image/${ext}`;
            attachments.push({
              type: 'image',
              source: { type: 'base64', media_type: mediaType, data: fileData.toString('base64') },
            });
          } else if (ext === 'pdf') {
            // PDF — Route A: extract text for prompt; Route B: also send as document block
            try {
              if (!pdf) { pdf = (await import('pdf-parse')).default; }
              const pdfData = await pdf(fileData);
              const pdfText = pdfData.text || '';
              if (pdfText.trim()) {
                const maxChars = 100_000;
                const truncated = pdfText.length > maxChars
                  ? pdfText.slice(0, maxChars) + `\n\n[... 文档已截断，共 ${pdfText.length} 字符]`
                  : pdfText;
                // Prepend extracted text to the prompt
                textPart = `[文件: ${filePath} (${pdfData.numpages}页)]\n\n${truncated}\n\n---\n${textPart}`;
              } else {
                // Text extraction failed, fall back to document block
                attachments.push({
                  type: 'document',
                  source: { type: 'base64', media_type: 'application/pdf', data: fileData.toString('base64') },
                });
              }
            } catch {
              // pdf-parse failed, fall back to document block
              attachments.push({
                type: 'document',
                source: { type: 'base64', media_type: 'application/pdf', data: fileData.toString('base64') },
              });
            }
          } else if (ext === 'docx') {
            // DOCX — Route A: extract text via mammoth
            try {
              if (!mammoth) { mammoth = (await import('mammoth')).default; }
              const result = await mammoth.extractRawText({ buffer: fileData });
              const docText = result.value || '';
              if (docText.trim()) {
                const maxChars = 100_000;
                const truncated = docText.length > maxChars
                  ? docText.slice(0, maxChars) + `\n\n[... 文档已截断，共 ${docText.length} 字符]`
                  : docText;
                textPart = `[文件: ${filePath}]\n\n${truncated}\n\n---\n${textPart}`;
              } else {
                console.log(`${FG_YELLOW}Warning: DOCX file is empty or text extraction failed${RESET}`);
              }
            } catch (e: any) {
              console.log(`${FG_YELLOW}Warning: Failed to extract DOCX text: ${e.message}${RESET}`);
            }
          } else if (ext === 'doc') {
            console.log(`${FG_YELLOW}Warning: .doc format not supported, please convert to .docx${RESET}`);
          }
          textPart = textPart.replace(match, '').trim();
        } catch {
          console.log(`${FG_YELLOW}Warning: Could not read ${filePath}${RESET}`);
        }
      }
      if (attachments.length > 0) {
        submitPayload = { prompt: textPart || '请分析这个文件', attachments };
        console.log(`${DIM}  📎 ${attachments.length} attachment(s)${RESET}`);
      } else if (textPart !== input) {
        // Text was extracted from documents and prepended to prompt
        submitPayload = { prompt: textPart };
        console.log(`${DIM}  📄 Document text extracted${RESET}`);
      }
    }

    try {
      await client.request('submitMessage', submitPayload);
    } catch (err) {
      stopSpinner();
      console.error(`${FG_RED}Request failed: ${err}${RESET}`);
    }

    rl.prompt();
  }

  rl.on('close', async () => {
    stopSpinner();
    console.log(`\n${DIM}Disconnected (daemon stays running).${RESET}`);
    await client.disconnect();
    process.exit(0);
  });

}

main().catch((err) => {
  stopSpinner();
  console.error(`${FG_RED}${BOLD}Fatal:${RESET} ${err.message}`);
  process.exit(1);
});
