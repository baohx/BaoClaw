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

  console.log(`${FG_ORANGE}${BOLD}  Welcome to BaoClaw ${RESET}${DIM}v0.10.0${RESET}`);
  console.log(`${FG_GRAY}${line}${RESET}`);
  console.log(`  ${DIM}Session${RESET}  ${sessionId}`);
  console.log(`  ${DIM}Model${RESET}    ${FG_GREEN}${model}${RESET}`);
  console.log(`  ${DIM}CWD${RESET}      ${cwd}`);
  console.log(`${FG_GRAY}${line}${RESET}`);
  console.log();
  console.log(`  ${DIM}Type your message and press Enter. /help for all commands.${RESET}`);
  console.log();
}

// ═══════════════════════════════════════════════════════════════
// Message formatting
// ═══════════════════════════════════════════════════════════════
function formatToolUse(toolName: string, input: unknown): string {
  const inp = (typeof input === 'object' && input !== null) ? input as Record<string, unknown> : {};

  // Smart formatting per tool type
  if (toolName === 'Bash') {
    const cmd = 'command' in inp ? String(inp.command) : '';
    const preview = cmd.length > 120 ? cmd.slice(0, 120) + '…' : cmd;
    return `  ${FG_MAGENTA}❯${RESET} ${FG_WHITE}${BOLD}$ ${preview}${RESET}`;
  }
  if (toolName === 'FileRead' || toolName === 'Read') {
    const fp = 'file_path' in inp ? String(inp.file_path) : '';
    return `  ${FG_BLUE}📄${RESET} ${DIM}read${RESET}  ${FG_WHITE}${fp}${RESET}`;
  }
  if (toolName === 'FileWrite' || toolName === 'Write') {
    const fp = 'file_path' in inp ? String(inp.file_path) : '';
    return `  ${FG_GREEN}✏️${RESET}  ${DIM}write${RESET} ${FG_WHITE}${fp}${RESET}`;
  }
  if (toolName === 'FileEdit' || toolName === 'Edit') {
    const fp = 'file_path' in inp ? String(inp.file_path) : '';
    return `  ${FG_YELLOW}✎${RESET}  ${DIM}edit${RESET}  ${FG_WHITE}${fp}${RESET}`;
  }
  if (toolName === 'Grep' || toolName === 'GrepTool') {
    const pattern = 'pattern' in inp ? String(inp.pattern) : '';
    const fp = 'path' in inp ? ` ${DIM}in${RESET} ${String(inp.path)}` : '';
    return `  ${FG_CYAN}🔍${RESET} ${DIM}grep${RESET}  ${FG_WHITE}/${pattern}/${RESET}${fp}`;
  }
  if (toolName === 'Glob' || toolName === 'GlobTool') {
    const pattern = 'pattern' in inp ? String(inp.pattern) : '';
    return `  ${FG_CYAN}📂${RESET} ${DIM}glob${RESET}  ${FG_WHITE}${pattern}${RESET}`;
  }
  if (toolName === 'WebFetchTool' || toolName === 'WebFetch') {
    const url = 'url' in inp ? String(inp.url) : '';
    const short = url.length > 80 ? url.slice(0, 80) + '…' : url;
    return `  ${FG_BLUE}🌐${RESET} ${DIM}fetch${RESET} ${FG_WHITE}${short}${RESET}`;
  }
  if (toolName === 'WebSearchTool' || toolName === 'Search' || toolName === 'WebSearch') {
    const q = 'query' in inp ? String(inp.query) : '';
    return `  ${FG_BLUE}🔎${RESET} ${DIM}search${RESET} ${FG_WHITE}${q}${RESET}`;
  }
  if (toolName === 'TodoWriteTool' || toolName === 'TodoWrite') {
    return `  ${FG_YELLOW}📝${RESET} ${DIM}todo${RESET}  ${FG_WHITE}updating todo list${RESET}`;
  }
  if (toolName === 'AgentTool' || toolName === 'Agent') {
    const prompt = 'prompt' in inp ? String(inp.prompt).slice(0, 80) : '';
    return `  ${FG_ORANGE}🤖${RESET} ${DIM}agent${RESET} ${FG_WHITE}${prompt}${prompt.length >= 80 ? '…' : ''}${RESET}`;
  }

  // MCP tools and other unknown tools — show name + compact params
  const paramKeys = Object.keys(inp);
  const paramPreview = paramKeys.length > 0
    ? paramKeys.slice(0, 3).map(k => {
        const v = String(inp[k] ?? '');
        return `${DIM}${k}=${RESET}${v.length > 40 ? v.slice(0, 40) + '…' : v}`;
      }).join(' ')
    : '';
  return `  ${FG_MAGENTA}⚡${RESET} ${FG_WHITE}${BOLD}${toolName}${RESET} ${paramPreview}`;
}

function formatToolResult(output: unknown, isError: boolean, toolName?: string, toolInput?: unknown): string {
  const prefix = isError ? `${FG_RED}✗${RESET}` : `${FG_GREEN}✓${RESET}`;

  if (typeof output === 'string') return formatResultText(output, isError, prefix);
  if (typeof output !== 'object' || output === null) {
    return `  ${prefix} ${isError ? FG_RED : FG_GRAY}${String(output)}${RESET}`;
  }

  const o = output as Record<string, unknown>;

  // ── Bash ──
  if (toolName === 'Bash') {
    const text = typeof o.output === 'string' ? o.output : typeof o.stdout === 'string' ? o.stdout : '';
    const exitCode = typeof o.exit_code === 'number' ? o.exit_code : null;
    if (!text.trim() && !isError) return `  ${prefix} ${DIM}(no output)${RESET}`;
    const exitSuffix = isError && exitCode !== null ? ` ${DIM}exit ${exitCode}${RESET}` : '';
    return formatResultText(text, isError, prefix) + exitSuffix;
  }

  // ── FileRead ──
  if (toolName === 'FileRead' || toolName === 'Read') {
    const linesRead = o.lines_read ?? o.total_lines ?? '';
    return `  ${prefix} ${DIM}${linesRead} lines${o.file_path ? ' from ' + o.file_path : ''}${RESET}`;
  }

  // ── FileWrite ──
  if (toolName === 'FileWrite' || toolName === 'Write') {
    return `  ${prefix} ${DIM}${o.file_path ?? ''}${o.bytes_written ? ' (' + o.bytes_written + ' bytes)' : ''}${RESET}`;
  }

  // ── FileEdit ──
  if (toolName === 'FileEdit' || toolName === 'Edit') {
    if (isError && typeof o.error === 'string') return `  ${prefix} ${FG_RED}${o.error}${RESET}`;
    return `  ${prefix} ${DIM}${o.file_path ?? ''}${RESET}`;
  }

  // ── GrepTool ──
  if (toolName === 'GrepTool' || toolName === 'Grep') {
    const matches = Array.isArray(o.matches) ? o.matches : [];
    const trunc = o.truncated ? ' (truncated)' : '';
    if (matches.length === 0) return `  ${prefix} ${DIM}no matches${RESET}`;
    return `  ${prefix} ${DIM}${matches.length} match${matches.length > 1 ? 'es' : ''}${trunc}${RESET}`;
  }

  // ── GlobTool ──
  if (toolName === 'GlobTool' || toolName === 'Glob') {
    const files = Array.isArray(o.files) ? o.files : [];
    if (files.length === 0) return `  ${prefix} ${DIM}no files found${RESET}`;
    const preview = files.slice(0, 4).map((f: unknown) => String(f)).join(', ');
    const more = files.length > 4 ? ` +${files.length - 4} more` : '';
    return `  ${prefix} ${DIM}${files.length} files: ${preview}${more}${RESET}`;
  }

  // ── WebFetchTool ──
  if (toolName === 'WebFetchTool' || toolName === 'WebFetch') {
    const content = typeof o.content === 'string' ? o.content : '';
    if (!content) return `  ${prefix} ${DIM}(empty response)${RESET}`;
    return `  ${prefix} ${DIM}${content.length.toLocaleString()} chars fetched${RESET}`;
  }

  // ── WebSearchTool — show results with titles and URLs ──
  if (toolName === 'WebSearchTool' || toolName === 'Search' || toolName === 'WebSearch') {
    const results = Array.isArray(o.results) ? o.results as Record<string, unknown>[] : [];
    if (results.length === 0) return `  ${prefix} ${DIM}no results${RESET}`;
    let out = `  ${prefix} ${DIM}${results.length} result${results.length !== 1 ? 's' : ''}${RESET}\n`;
    for (const r of results.slice(0, 5)) {
      const title = typeof r.title === 'string' ? r.title : '';
      const url = typeof r.url === 'string' ? r.url : '';
      const snippet = typeof r.snippet === 'string' ? r.snippet : '';
      const shortTitle = title.length > 60 ? title.slice(0, 60) + '…' : title;
      const shortSnippet = snippet.length > 80 ? snippet.slice(0, 80) + '…' : snippet;
      out += `    ${FG_WHITE}${shortTitle}${RESET}\n`;
      out += `    ${FG_BLUE}${UNDERLINE}${url}${RESET}\n`;
      if (shortSnippet) out += `    ${DIM}${shortSnippet}${RESET}\n`;
    }
    if (results.length > 5) out += `    ${DIM}… +${results.length - 5} more${RESET}\n`;
    return out.trimEnd();
  }

  // ── AgentTool — show result text with cost ──
  if (toolName === 'AgentTool' || toolName === 'Agent') {
    const text = typeof o.result === 'string' ? o.result : '';
    const costVal = typeof o.cost_usd === 'number' ? o.cost_usd as number : 0;
    const cost = costVal > 0 ? ` ${DIM}(` + '$' + `${costVal.toFixed(4)})${RESET}` : '';
    if (text) return formatResultText(text, isError, prefix) + cost;
    return `  ${prefix} ${DIM}done${RESET}${cost}`;
  }

  // ── Simple confirmation tools ──
  if (['TodoWriteTool', 'TodoWrite', 'MemoryTool', 'Memory',
       'ProjectNoteTool', 'ProjectNote', 'SaveProjectRule',
       'NotebookEditTool', 'NotebookEdit'].includes(toolName || '')) {
    if (isError && typeof o.error === 'string') return `  ${prefix} ${FG_RED}${o.error}${RESET}`;
    return `  ${prefix} ${DIM}done${RESET}`;
  }

  // ── ToolSearchTool ──
  if (toolName === 'ToolSearchTool' || toolName === 'ToolSearch') {
    const matches = Array.isArray(o.matches) ? o.matches : [];
    if (matches.length === 0) return `  ${prefix} ${DIM}no matching tools${RESET}`;
    const names = matches.slice(0, 5).map((m: any) => m?.name || m).join(', ');
    const more = matches.length > 5 ? ` +${matches.length - 5}` : '';
    return `  ${prefix} ${DIM}${names}${more}${RESET}`;
  }

  // ── Evolve ──
  if (toolName === 'Evolve' || toolName === 'EvolveTool') {
    if (o.created) return `  ${prefix} ${DIM}skill created${RESET}`;
    if (o.improved) return `  ${prefix} ${DIM}skill improved${RESET}`;
    if (o.promoted) return `  ${prefix} ${DIM}skill promoted${RESET}`;
    if (typeof o.exported === 'number') return `  ${prefix} ${DIM}${o.exported} skills exported${RESET}`;
    if (Array.isArray(o.candidates)) return `  ${prefix} ${DIM}${(o.candidates as any[]).length} candidates${RESET}`;
    return `  ${prefix} ${DIM}done${RESET}`;
  }

  // ── Generic fallback ──
  if (Array.isArray(o.content)) {
    const textParts = (o.content as any[]).filter((c: any) => c?.type === 'text' && typeof c?.text === 'string').map((c: any) => c.text as string);
    if (textParts.length > 0) return formatResultText(textParts.join('\n'), isError, prefix);
    const imgCount = (o.content as any[]).filter((c: any) => c?.type === 'image').length;
    if (imgCount > 0) return `  ${prefix} ${DIM}${imgCount} image${imgCount > 1 ? 's' : ''}${RESET}`;
  }
  const textField = o.output ?? o.stdout ?? o.content ?? o.result ?? o.text ?? o.message;
  if (typeof textField === 'string' && textField.trim()) return formatResultText(textField, isError, prefix);
  for (const key of Object.keys(o)) {
    if (Array.isArray(o[key])) return `  ${prefix} ${DIM}${(o[key] as unknown[]).length} ${key}${RESET}`;
  }
  const compact = JSON.stringify(output);
  if (compact.length <= 100) return `  ${prefix} ${FG_GRAY}${compact}${RESET}`;
  return `  ${prefix} ${FG_GRAY}${compact.slice(0, 100)}…${RESET}`;
}

/** Format a text result with truncation and coloring */
function formatResultText(text: string, isError: boolean, prefix: string): string {
  text = text.replace(/[A-Za-z0-9+/=]{500,}/g, '[binary data]');
  const color = isError ? FG_RED : FG_GRAY;
  let lines = text.split('\n');
  while (lines.length > 0 && !lines[lines.length - 1].trim()) lines.pop();
  if (lines.length > 5) { const t = lines.length; lines = lines.slice(0, 5); lines.push(`${DIM}… (${t - 5} more lines)${RESET}`); }
  let truncated = lines.join('\n');
  if (truncated.length > 300) truncated = truncated.slice(0, 300) + `${DIM}…${RESET}`;
  if (!truncated.includes('\n')) return `  ${prefix} ${color}${truncated}${RESET}`;
  return `  ${prefix}\n${truncated.split('\n').map(l => `  ${color}  ${l}${RESET}`).join('\n')}`;
}

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
      const dir = d.cwd.length > 40 ? '…' + d.cwd.slice(-39) : d.cwd;
      console.log(`  ${FG_WHITE}${BOLD}${i + 1}${RESET}  ${FG_WHITE}${dir}${RESET}  ${DIM}pid=${d.pid} · ${age} · ${d.session_id.slice(0, 8)}${RESET}`);
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
  '/projects', '/cron', '/history',
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
  // Track tool_use_id → tool_name for smart result formatting
  const pendingTools = new Map<string, { name: string; input: unknown }>();

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
        if (isStreaming) {
          // Flush accumulated text before showing tool use
          if (currentText.trim()) {
            process.stdout.write(`\n${FG_ORANGE}${BOLD}BaoClaw${RESET}\n`);
            process.stdout.write(renderMarkdown(currentText));
            process.stdout.write('\n');
            currentText = '';
          }
          isStreaming = false;
        }
        toolCount++;
        const tu = event as { tool_name: string; input: unknown; tool_use_id: string };
        pendingTools.set(tu.tool_use_id, { name: tu.tool_name, input: tu.input });
        console.log(formatToolUse(tu.tool_name, tu.input));
        startSpinner(`${tu.tool_name}…`);
        break;
      }

      case 'tool_result': {
        stopSpinner();
        const tr = event as { tool_use_id: string; output: unknown; is_error: boolean };
        const toolInfo = pendingTools.get(tr.tool_use_id);
        pendingTools.delete(tr.tool_use_id);
        console.log(formatToolResult(tr.output, tr.is_error, toolInfo?.name, toolInfo?.input));
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

        // Show a compact permission prompt
        const inp = pr.input || {};
        const paramPreview = Object.keys(inp).slice(0, 2).map(k => {
          const v = String(inp[k] ?? '');
          return `${k}=${v.length > 30 ? v.slice(0, 30) + '…' : v}`;
        }).join(', ');

        console.log(`\n  ${FG_YELLOW}⚠ Permission${RESET}  ${FG_WHITE}${BOLD}${pr.tool_name}${RESET}  ${DIM}${paramPreview}${RESET}`);
        console.log(`    ${FG_GREEN}[y]${RESET} Allow  ${FG_GREEN}[a]${RESET} Always  ${FG_RED}[n]${RESET} Deny`);

        const permRl = readline.createInterface({ input: process.stdin, output: process.stdout });
        permRl.question(`  ${FG_ORANGE}> ${RESET}`, async (answer: string) => {
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
        } else if (queryStartTime > 0 && !currentText.trim() && toolCount === 0) {
          // AI returned without any text or tool use — show a hint
          const r = event as { status?: string };
          if (r.status === 'complete') {
            console.log(`\n${DIM}  (empty response — try rephrasing or providing more context)${RESET}\n`);
          }
        }
        // Only show stats bar for actual queries (skip stale/duplicate events)
        if (queryStartTime > 0) {
          const result = event as { status: string; num_turns: number; duration_ms: number; usage?: { input_tokens: number; output_tokens: number }; total_cost_usd?: number };
          const elapsed = Date.now() - queryStartTime;
          const elapsedStr = elapsed >= 60000
            ? `${(elapsed / 60000).toFixed(1)}m`
            : `${(elapsed / 1000).toFixed(1)}s`;
  
          // Build a clean stats line with separators
          const statParts: string[] = [];
          if (toolCount > 0) {
            statParts.push(`${FG_MAGENTA}⚡ ${toolCount} tool${toolCount > 1 ? 's' : ''}${RESET}`);
          }
          if (result.usage && (result.usage.input_tokens > 0 || result.usage.output_tokens > 0)) {
            const inp = result.usage.input_tokens >= 1000
              ? `${(result.usage.input_tokens / 1000).toFixed(1)}k`
              : `${result.usage.input_tokens}`;
            const out = result.usage.output_tokens >= 1000
              ? `${(result.usage.output_tokens / 1000).toFixed(1)}k`
              : `${result.usage.output_tokens}`;
            statParts.push(`${FG_CYAN}↑${inp} ↓${out}${RESET}`);
          }
          if (result.total_cost_usd && result.total_cost_usd > 0) {
            statParts.push(`${FG_YELLOW}$${result.total_cost_usd.toFixed(4)}${RESET}`);
          }
          statParts.push(`${FG_GRAY}${elapsedStr}${RESET}`);
  
          const statsLine = statParts.join(`${FG_GRAY} │ ${RESET}`);
          console.log(`\n${FG_GRAY}  ─${RESET} ${statsLine} ${FG_GRAY}─${RESET}\n`);
        }
        // Always reset state
        currentText = '';
        toolCount = 0;
        queryStartTime = 0;
        break;
      }

      case 'model_fallback': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
        const fb = event as { from_model: string; to_model: string };
        console.log(`\n  ${FG_YELLOW}⚠ Fallback${RESET} ${DIM}${fb.from_model}${RESET} ${FG_YELLOW}→${RESET} ${FG_GREEN}${fb.to_model}${RESET} ${DIM}(rate limited)${RESET}\n`);
        startSpinner(fb.to_model + '…');
        break;
      }

      case 'error': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
        const err = event as { code: string; message: string };
        console.log(`\n  ${FG_RED}✗ ${BOLD}${err.code || 'Error'}${RESET}${FG_RED}: ${err.message}${RESET}\n`);
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

        // Group by type
        const groups: Record<string, typeof result.tools> = {};
        for (const tool of result.tools) {
          const t = tool.type || 'other';
          if (!groups[t]) groups[t] = [];
          groups[t].push(tool);
        }

        for (const [type, tools] of Object.entries(groups)) {
          const badge = type === 'builtin' ? `${FG_GREEN}${type}${RESET}` : `${FG_BLUE}${type}${RESET}`;
          console.log(`  ${FG_GRAY}── ${badge} ${FG_GRAY}(${tools.length}) ──${RESET}`);
          for (const tool of tools) {
            const desc = tool.description
              ? (tool.description.length > 60 ? tool.description.slice(0, 60) + '…' : tool.description)
              : '';
            console.log(`  ${FG_WHITE}${tool.name}${RESET}  ${DIM}${desc}${RESET}`);
          }
          console.log();
        }
      } catch (err) {
        console.error(`${FG_RED}Failed to list tools: ${err}${RESET}`);
      }
      rl.prompt();
      return;
    }

    if (input === '/mcp') {
      try {
        const result = await client.request<{ servers: Array<{ name: string; command?: string; args?: string[]; server_type: string; url?: string; disabled: boolean; source: string; config_path: string }>; count: number }>('listMcpServers');
        if (result.count === 0) {
          console.log(`\n${DIM}No MCP servers configured.${RESET}`);
          console.log(`${DIM}Add servers to .baoclaw/mcp.json or ~/.baoclaw/mcp.json${RESET}\n`);
        } else {
          console.log(`\n${FG_ORANGE}${BOLD}MCP Servers${RESET} ${DIM}(${result.count})${RESET}\n`);
          for (const srv of result.servers) {
            const statusIcon = srv.disabled ? `${FG_RED}●${RESET}` : `${FG_GREEN}●${RESET}`;
            const source = `${DIM}[${srv.source}]${RESET}`;
            console.log(`  ${statusIcon} ${FG_WHITE}${BOLD}${srv.name}${RESET} ${source}`);
            if (srv.command) {
              const args = srv.args?.join(' ') || '';
              const cmd = `${srv.command} ${args}`.trim();
              const short = cmd.length > 60 ? cmd.slice(0, 60) + '…' : cmd;
              console.log(`    ${DIM}${srv.server_type}: ${short}${RESET}`);
            } else if (srv.url) {
              console.log(`    ${DIM}${srv.server_type}: ${srv.url}${RESET}`);
            }
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

        console.log(`\n${FG_ORANGE}${BOLD}Model${RESET}\n`);
        console.log(`  ${FG_WHITE}Active:${RESET}   ${FG_GREEN}${activeModel}${RESET}`);
        if (process.env.ANTHROPIC_MODEL) {
          console.log(`  ${DIM}(env override, config: ${configModel})${RESET}`);
        }
        console.log(`  ${FG_WHITE}Retries:${RESET}  ${maxRetries} per model`);

        if (fallbackModels.length > 0) {
          console.log();
          console.log(`  ${FG_GRAY}── Fallback Chain ──${RESET}`);
          console.log(`  ${FG_CYAN}0${RESET}  ${FG_GREEN}${activeModel}${RESET}  ${DIM}primary${RESET}`);
          fallbackModels.forEach((m: string, i: number) => {
            console.log(`  ${FG_CYAN}${i + 1}${RESET}  ${FG_YELLOW}${m}${RESET}`);
          });
        } else {
          console.log(`\n  ${DIM}No fallback models. Edit ~/.baoclaw/config.json${RESET}`);
        }

        console.log(`\n  ${DIM}Switch: /model <name>${RESET}\n`);
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

    if (input.startsWith('/projects')) {
      const projArgs = input.slice('/projects'.length).trim();

      if (!projArgs || projArgs === 'list') {
        try {
          const result = await client.request<{ projects: any[]; count: number }>('projectsList');
          if (result.count === 0) {
            console.log(`\n${DIM}No projects registered. Use /projects new <path> [description]${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Projects${RESET} ${DIM}(${result.count})${RESET}\n`);
            // Calculate column widths
            const idWidth = Math.max(4, ...result.projects.map((p: any) => (p.id || '').length));
            const descWidth = Math.max(8, ...result.projects.map((p: any) => (p.description || '').length));
            const clampedDesc = Math.min(descWidth, 30);

            for (const p of result.projects) {
              const id = (p.id || '').padEnd(idWidth);
              const desc = (p.description || '').slice(0, 30).padEnd(clampedDesc);
              const last = p.last_accessed ? timeSince(p.last_accessed) : 'never';
              const sid = p.session_id ? `${DIM}session:${p.session_id}${RESET}` : '';
              console.log(`  ${FG_CYAN}${id}${RESET}  ${FG_WHITE}${BOLD}${desc}${RESET}  ${DIM}${last}${RESET}  ${sid}`);
              console.log(`  ${' '.repeat(idWidth)}  ${DIM}${p.cwd}${RESET}`);
            }
            console.log(`\n  ${DIM}Switch: /projects <id>  ·  New: /projects new <path> [desc]${RESET}\n`);
          }
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (projArgs.startsWith('new ')) {
        const rest = projArgs.slice(4).trim();
        const spaceIdx = rest.indexOf(' ');
        let targetPath: string;
        let desc: string | undefined;
        if (spaceIdx > 0) {
          targetPath = rest.slice(0, spaceIdx);
          desc = rest.slice(spaceIdx + 1).trim() || undefined;
        } else {
          targetPath = rest;
        }
        if (!targetPath) {
          console.log(`\n${FG_YELLOW}Usage: /projects new <path> [description]${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const params: Record<string, unknown> = { cwd: targetPath };
          if (desc) params.description = desc;
          const result = await client.request<{ project: any; switched: boolean }>('projectsNew', params);
          try { process.chdir(result.project.cwd); } catch {}
          console.log(`\n${FG_GREEN}${BOLD}Created & switched to${RESET} ${result.project.description}`);
          console.log(`${DIM}  [${result.project.id}] ${result.project.cwd}${RESET}`);
          currentText = ''; isStreaming = false; toolCount = 0;
          console.log();
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (projArgs.startsWith('desc ')) {
        const parts = projArgs.slice(5).trim().split(/\s+/);
        const idPrefix = parts[0];
        const newDesc = parts.slice(1).join(' ');
        if (!idPrefix || !newDesc) {
          console.log(`\n${FG_YELLOW}Usage: /projects desc <id> <description>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          await client.request('projectsUpdateDesc', { id_prefix: idPrefix, description: newDesc });
          console.log(`\n${FG_GREEN}✓ Description updated${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      // /projects <id_prefix> — switch
      const idPrefix = projArgs;
      try {
        const result = await client.request<{ project: any; message_count: number }>('projectsSwitch', { id_prefix: idPrefix });
        try { process.chdir(result.project.cwd); } catch {}
        console.log(`\n${FG_GREEN}${BOLD}Switched to${RESET} ${result.project.description}`);
        console.log(`${DIM}  [${result.project.id}] ${result.project.cwd}${RESET}`);
        if (result.message_count > 0) {
          console.log(`${DIM}  Resumed session (${result.message_count} messages)${RESET}`);
        } else {
          console.log(`${DIM}  Fresh session${RESET}`);
        }
        currentText = ''; isStreaming = false; toolCount = 0;
        console.log();
      } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
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
              const statusIcon = j.enabled ? `${FG_GREEN}●${RESET}` : `${FG_RED}●${RESET}`;
              const last = j.last_run ? timeSince(j.last_run) : 'never';
              const prompt = j.prompt.length > 60 ? j.prompt.slice(0, 60) + '…' : j.prompt;
              console.log(`  ${statusIcon} ${FG_WHITE}${j.id}${RESET}  ${j.name}  ${DIM}${j.schedule}${RESET}  ${DIM}last: ${last}${RESET}`);
              console.log(`    ${DIM}${prompt}${RESET}`);
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

    if (input.startsWith('/history')) {
      const arg = input.slice('/history'.length).trim();
      const count = parseInt(arg, 10) || 10;
      try {
        const result = await client.request<{ messages: any[]; count: number; total: number }>('talkTail', { count });
        console.log(`\n${FG_ORANGE}${BOLD}Recent History${RESET} ${DIM}(${result.count} of ${result.total})${RESET}\n`);
        for (const m of result.messages) {
          const ts = m.timestamp ? `${DIM}${m.timestamp.slice(11, 19)}${RESET}` : '';
          if (m.role === 'user') {
            const preview = (m.text || '').slice(0, 100);
            console.log(`  ${ts} ${FG_BRIGHT_WHITE}${BOLD}You${RESET}  ${preview}${preview.length >= 100 ? '…' : ''}`);
          } else if (m.role === 'assistant') {
            const preview = (m.text || '').slice(0, 100);
            const toolBadge = m.tools && m.tools.length > 0
              ? ` ${FG_MAGENTA}[${m.tools.length} tool${m.tools.length > 1 ? 's' : ''}]${RESET}`
              : '';
            console.log(`  ${ts} ${FG_ORANGE}${BOLD}BC${RESET}${toolBadge}  ${DIM}${preview}${preview.length >= 100 ? '…' : ''}${RESET}`);
          } else {
            console.log(`  ${ts} ${DIM}[system]${RESET}`);
          }
        }
        console.log();
      } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
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
          const pct = ((result.tokens_saved / result.tokens_before) * 100).toFixed(0);
          console.log(`\n${FG_GREEN}${BOLD}Compacted${RESET}`);
          console.log(`  ${FG_WHITE}Before:${RESET}  ${result.tokens_before.toLocaleString()} tokens`);
          console.log(`  ${FG_WHITE}After:${RESET}   ${result.tokens_after.toLocaleString()} tokens`);
          console.log(`  ${FG_WHITE}Saved:${RESET}   ${FG_GREEN}${result.tokens_saved.toLocaleString()} tokens (${pct}%)${RESET}`);
          console.log(`  ${FG_WHITE}Summary:${RESET} ${result.summary_tokens.toLocaleString()} tokens`);
          console.log();
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
              const catColor = m.category === 'preference' ? FG_MAGENTA
                : m.category === 'decision' ? FG_YELLOW
                : FG_CYAN;
              const content = m.content.length > 80 ? m.content.slice(0, 80) + '…' : m.content;
              console.log(`  ${catColor}${m.category.padEnd(10)}${RESET} ${FG_WHITE}${content}${RESET}  ${DIM}[${m.id}]${RESET}`);
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
              const statusIcon = statusStr === 'Running' ? `${FG_YELLOW}●${RESET}`
                : statusStr === 'Completed' ? `${FG_GREEN}●${RESET}`
                : statusStr === 'Aborted' ? `${FG_GRAY}●${RESET}`
                : `${FG_RED}●${RESET}`;
              const desc = t.description.length > 50 ? t.description.slice(0, 50) + '…' : t.description;
              console.log(`  ${statusIcon} ${FG_WHITE}${t.id}${RESET}  ${desc}  ${DIM}${statusStr}${RESET}`);
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
          const statusColor = statusStr === 'Running' ? FG_YELLOW
            : statusStr === 'Completed' ? FG_GREEN
            : statusStr.startsWith('Failed') ? FG_RED : FG_GRAY;
          console.log(`\n${FG_ORANGE}${BOLD}Task${RESET} ${FG_WHITE}${t.id}${RESET}`);
          console.log(`  ${FG_WHITE}Status:${RESET}  ${statusColor}${statusStr}${RESET}`);
          console.log(`  ${FG_WHITE}Desc:${RESET}    ${t.description}`);
          console.log(`  ${FG_WHITE}Created:${RESET} ${DIM}${t.created_at}${RESET}`);
          if (t.completed_at) console.log(`  ${FG_WHITE}Done:${RESET}    ${DIM}${t.completed_at}${RESET}`);
          if (t.result) {
            const preview = t.result.length > 150 ? t.result.slice(0, 150) + '…' : t.result;
            console.log(`  ${FG_WHITE}Result:${RESET}  ${DIM}${preview}${RESET}`);
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

      console.log(`  ${FG_GRAY}── Conversation ──${RESET}`);
      console.log(`  ${FG_WHITE}/compact${RESET}    ${DIM}Compress conversation context${RESET}`);
      console.log(`  ${FG_WHITE}/think${RESET}      ${DIM}Toggle extended thinking mode${RESET}`);
      console.log(`  ${FG_WHITE}/model${RESET}      ${DIM}Show or switch model${RESET}`);
      console.log(`  ${FG_WHITE}/history${RESET}    ${DIM}Recent conversation: /history [n]${RESET}`);
      console.log(`  ${FG_WHITE}/abort${RESET}      ${DIM}Cancel current request${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Projects & Git ──${RESET}`);
      console.log(`  ${FG_WHITE}/projects${RESET}   ${DIM}List, switch, create projects${RESET}`);
      console.log(`  ${FG_WHITE}/git${RESET}        ${DIM}Git status (branch, changes)${RESET}`);
      console.log(`  ${FG_WHITE}/diff${RESET}       ${DIM}Git diff summary${RESET}`);
      console.log(`  ${FG_WHITE}/commit${RESET}     ${DIM}Stage all and commit${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Tools & Extensions ──${RESET}`);
      console.log(`  ${FG_WHITE}/tools${RESET}      ${DIM}List registered tools${RESET}`);
      console.log(`  ${FG_WHITE}/mcp${RESET}        ${DIM}List MCP servers${RESET}`);
      console.log(`  ${FG_WHITE}/skills${RESET}     ${DIM}List discovered skills${RESET}`);
      console.log(`  ${FG_WHITE}/plugins${RESET}    ${DIM}List discovered plugins${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Automation ──${RESET}`);
      console.log(`  ${FG_WHITE}/task${RESET}       ${DIM}Background tasks: run, list, status, stop${RESET}`);
      console.log(`  ${FG_WHITE}/cron${RESET}       ${DIM}Scheduled tasks: add, list, remove, toggle${RESET}`);
      console.log(`  ${FG_WHITE}/memory${RESET}     ${DIM}Long-term memory: list, add, delete, clear${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Input & Integrations ──${RESET}`);
      console.log(`  ${FG_WHITE}/voice${RESET}      ${DIM}Voice input (requires whisper.cpp)${RESET}`);
      console.log(`  ${FG_WHITE}@file.pdf${RESET}   ${DIM}Attach file: @photo.png @doc.pdf @doc.docx${RESET}`);
      console.log(`  ${FG_WHITE}/telegram${RESET}   ${DIM}Manage Telegram gateway${RESET}`);
      console.log(`  ${FG_WHITE}/telemetry${RESET}  ${DIM}Toggle telemetry: on|off${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Session ──${RESET}`);
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
