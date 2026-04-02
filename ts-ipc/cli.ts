#!/usr/bin/env node
/**
 * BaoClaw CLI — Rich terminal interface powered by Rust core engine.
 * Visual style inspired by Claude Code's TUI.
 */
import * as net from 'net';
import * as readline from 'readline';
import * as path from 'path';
import { spawn, ChildProcess } from 'child_process';
import { renderMarkdown } from './markdownRenderer';
import * as fs from 'fs';
import * as os from 'os';

// ═══════════════════════════════════════════════════════════════
// ANSI helpers
// ═══════════════════════════════════════════════════════════════
const ESC = '\x1b[';
const RESET = `${ESC}0m`;
const BOLD = `${ESC}1m`;
const DIM = `${ESC}2m`;
const ITALIC = `${ESC}3m`;
const UNDERLINE = `${ESC}4m`;

// Colors (Claude-inspired palette)
const FG_ORANGE = `${ESC}38;2;217;119;40m`;   // Claude orange
const FG_CYAN = `${ESC}36m`;
const FG_GREEN = `${ESC}32m`;
const FG_YELLOW = `${ESC}33m`;
const FG_RED = `${ESC}31m`;
const FG_MAGENTA = `${ESC}35m`;
const FG_BLUE = `${ESC}34m`;
const FG_WHITE = `${ESC}37m`;
const FG_GRAY = `${ESC}90m`;
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
  // White Bichon Frise dog — BaoClaw mascot
  const W = `${ESC}38;2;255;255;255m`;  // white
  const B = `${ESC}38;2;40;40;40m`;     // black (eyes/nose)
  const P = `${ESC}38;2;255;182;193m`;  // pink (tongue)
  const S = `${ESC}38;2;220;220;220m`;  // light shadow
  const G = FG_GRAY;
  const O = FG_ORANGE;
  const R = RESET;

  const logo = `
${G}                                                                ${R}
${G}        ${W}░░░░░░░░░░░${R}${G}                                             ${R}
${G}      ${W}░░░░░░░░░░░░░░░${R}${G}                                           ${R}
${G}     ${W}░░░░░░░░░░░░░░░░░${R}${G}        ${O}╔╗   ╔╗${R}${G}                        ${R}
${G}    ${W}░░░░░${R}${B}██${R}${W}░░░░░${R}${B}██${R}${W}░░░░${R}${G}        ${O}║╚╗╔╝║${R}${G}                        ${R}
${G}    ${W}░░░░░░░░${R}${B}▄${R}${W}░░░░░░░░░${R}${G}        ${O}╚═╝╚═╝${R}${G}                        ${R}
${G}    ${W}░░░░░░░${R}${P}▀▀▀${R}${W}░░░░░░░░${R}${G}                                       ${R}
${G}     ${W}░░░░░░░░░░░░░░░░░${R}${G}    ${O}${BOLD}B a o C l a w${R}${G}                    ${R}
${G}    ${W}░░░░░░░░░░░░░░░░░░░${R}${G}                                         ${R}
${G}   ${W}░░░░░${R}${G}  ${W}░░░░░░░${R}${G}  ${W}░░░░░${R}${G}   ${S}AI Coding Assistant${R}${G}                ${R}
${G}   ${W}░░░░${R}${G}    ${W}░░░░░${R}${G}    ${W}░░░░${R}${G}   ${S}Powered by Rust${R}${G}                  ${R}
${G}    ${W}░░${R}${G}      ${W}░░░${R}${G}      ${W}░░${R}${G}                                       ${R}
${G}                                                                ${R}
`;
  process.stdout.write(logo);
}

function printWelcome(sessionId: string) {
  const cols = process.stdout.columns || 80;
  const line = '─'.repeat(Math.min(cols - 2, 70));

  console.log(`${FG_ORANGE}${BOLD}  Welcome to BaoClaw ${RESET}${DIM}v0.1.0${RESET}`);
  console.log(`${FG_GRAY}${line}${RESET}`);
  console.log(`${DIM}  Session: ${sessionId}${RESET}`);
  console.log(`${DIM}  cwd: ${process.cwd()}${RESET}`);
  console.log(`${FG_GRAY}${line}${RESET}`);
  console.log();
  console.log(`${DIM}  Tips: Type your message and press Enter.${RESET}`);
  console.log(`${DIM}        /tools    — list registered tools${RESET}`);
  console.log(`${DIM}        /mcp      — list MCP servers${RESET}`);
  console.log(`${DIM}        /skills   — list skills${RESET}`);
  console.log(`${DIM}        /plugins  — list plugins${RESET}`);
  console.log(`${DIM}        /compact  — compress conversation context${RESET}`);
  console.log(`${DIM}        /think    — toggle extended thinking${RESET}`);
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
  const color = isError ? FG_RED : FG_GRAY;
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
    }, 10000);

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
  '/clear', '/abort', '/task', '/voice', '/telemetry',
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
  const defaultBin = path.resolve(process.cwd(), 'claude-core', 'target', 'release', 'claude-core');
  const binaryPath = path.resolve(process.env.CLAUDE_CORE_BIN ?? defaultBin);

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
  const daemons = discoverDaemons();
  let socketPath: string;
  let child: ChildProcess | null = null;
  let isReconnect = false;

  if (daemons.length > 0) {
    const selected = await selectDaemon(daemons);
    if (selected) {
      // Connect to existing daemon
      socketPath = selected.socket;
      isReconnect = true;
      console.log(`${DIM}Reconnecting to pid=${selected.pid}...${RESET}`);
    } else {
      // Start new daemon
      socketPath = await startNewDaemon(binaryPath);
    }
  } else {
    socketPath = await startNewDaemon(binaryPath);
  }

  // Connect IPC
  const client = new IpcClient();
  await client.connect(socketPath);

  // Initialize
  const thinkingSettings = thinkingEnabled
    ? { thinking: { mode: 'enabled', budget_tokens: thinkingBudget } }
    : {};
  const initResult = await client.request<{ capabilities: Record<string, unknown>; session_id: string; reconnected?: boolean; message_count?: number }>(
    'initialize',
    { cwd: process.cwd(), settings: { ...thinkingSettings } }
  );

  stopSpinner();

  if (initResult.reconnected) {
    console.log(`\n${FG_GREEN}${BOLD}Reconnected${RESET} ${DIM}to session ${initResult.session_id} (${initResult.message_count} messages in history)${RESET}\n`);
  }
  printWelcome(initResult.session_id);

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
        process.stdout.write(`${DIM}${content}${RESET}`);
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
        console.log(`\n${FG_GRAY}  ${parts}${RESET}\n`);
        break;
      }

      case 'error': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
        const err = event as { code: string; message: string };
        console.log(`\n${FG_RED}${BOLD}Error${RESET}${FG_RED} [${err.code}]: ${err.message}${RESET}\n`);
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

  rl.prompt();

  rl.on('line', async (line: string) => {
    const input = line.trim();
    if (!input) { rl.prompt(); return; }

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

    if (input === '/compact') {
      startSpinner('Compacting conversation...');
      try {
        const result = await client.request<{ tokens_saved: number; summary_tokens: number }>('compact');
        stopSpinner();
        if (result.tokens_saved === 0) {
          console.log(`\n${DIM}Not enough messages to compact.${RESET}\n`);
        } else {
          console.log(`\n${FG_GREEN}${BOLD}Compacted${RESET} ${DIM}saved ${result.tokens_saved} tokens (summary: ${result.summary_tokens} tokens)${RESET}\n`);
        }
      } catch (err) {
        stopSpinner();
        console.error(`${FG_RED}Failed to compact: ${err}${RESET}`);
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
      console.log(`${FG_YELLOW}Voice input requires whisper.cpp integration.${RESET}`);
      console.log(`${DIM}This feature is planned for a future release.${RESET}`);
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
      console.log(`  ${FG_WHITE}/think${RESET}      ${DIM}Toggle extended thinking mode${RESET}`);
      console.log(`  ${FG_WHITE}/diff${RESET}       ${DIM}Show git diff summary${RESET}`);
      console.log(`  ${FG_WHITE}/commit${RESET}     ${DIM}Stage all and commit: /commit <message>${RESET}`);
      console.log(`  ${FG_WHITE}/git${RESET}        ${DIM}Show git status (branch, changes)${RESET}`);
      console.log(`  ${FG_WHITE}/task${RESET}       ${DIM}Background tasks: run, list, status, stop${RESET}`);
      console.log(`  ${FG_WHITE}/voice${RESET}      ${DIM}Voice input (requires whisper.cpp)${RESET}`);
      console.log(`  ${FG_WHITE}/telemetry${RESET}  ${DIM}Toggle telemetry: /telemetry on|off${RESET}`);
      console.log(`  ${FG_WHITE}/abort${RESET}      ${DIM}Cancel current request${RESET}`);
      console.log(`  ${FG_WHITE}/clear${RESET}      ${DIM}Clear screen${RESET}`);
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

    // Check for @file references (e.g., @image.png) and convert to content blocks
    let submitPayload: Record<string, unknown> = { prompt: input };
    const atMatches = input.match(/@(\S+\.(png|jpg|jpeg|gif|webp))/gi);
    if (atMatches) {
      const contentBlocks: Array<Record<string, unknown>> = [];
      let textPart = input;
      for (const match of atMatches) {
        const filePath = match.slice(1); // remove @
        const absPath = path.resolve(process.cwd(), filePath);
        try {
          const fileData = fs.readFileSync(absPath);
          const base64Data = fileData.toString('base64');
          const ext = path.extname(filePath).toLowerCase().slice(1);
          const mediaType = ext === 'jpg' ? 'image/jpeg' : `image/${ext}`;
          contentBlocks.push({
            type: 'image',
            source: {
              type: 'base64',
              media_type: mediaType,
              data: base64Data,
            },
          });
          textPart = textPart.replace(match, '').trim();
        } catch {
          console.log(`${FG_YELLOW}Warning: Could not read ${filePath}${RESET}`);
        }
      }
      if (textPart) {
        contentBlocks.unshift({ type: 'text', text: textPart });
      }
      if (contentBlocks.length > 0) {
        submitPayload = { prompt: contentBlocks };
      }
    }

    try {
      await client.request('submitMessage', submitPayload);
    } catch (err) {
      stopSpinner();
      console.error(`${FG_RED}Request failed: ${err}${RESET}`);
    }

    rl.prompt();
  });

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
