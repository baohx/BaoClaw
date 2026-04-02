#!/usr/bin/env node
/**
 * BaoClaw CLI — Rich terminal interface powered by Rust core engine.
 * Visual style inspired by Claude Code's TUI.
 */
import * as net from 'net';
import * as readline from 'readline';
import * as path from 'path';
import { spawn, ChildProcess } from 'child_process';

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
  const logo = `
${FG_GRAY}                                                          ${RESET}
${FG_GRAY}     *                                       ${FG_ORANGE}█████▓▓░${RESET}${FG_GRAY}     ${RESET}
${FG_GRAY}                                 *         ${FG_ORANGE}███▓░${RESET}${FG_GRAY}     ░░   ${RESET}
${FG_GRAY}            ░░░░░░                        ${FG_ORANGE}███▓░${RESET}${FG_GRAY}           ${RESET}
${FG_GRAY}    ░░░   ░░░░░░░░░░                      ${FG_ORANGE}███▓░${RESET}${FG_GRAY}           ${RESET}
${FG_GRAY}   ░░░░░░░░░░░░░░░░░░░    ${BOLD}*${RESET}${FG_GRAY}                ${FG_ORANGE}██▓░░${RESET}${FG_GRAY}      ▓   ${RESET}
${FG_GRAY}                                             ░▓▓${FG_ORANGE}███${RESET}${FG_GRAY}▓▓░    ${RESET}
${FG_GRAY} *                                 ░░░░                   ${RESET}
${FG_GRAY}                                 ░░░░░░░░                 ${RESET}
${FG_GRAY}                               ░░░░░░░░░░░░░░░░           ${RESET}
${FG_GRAY}      ${FG_CLAWD} █████████ ${RESET}${FG_GRAY}                         ▒▒░░▒▒      ▒ ▒▒${RESET}
${FG_GRAY}                                            ▒▒         ▒▒ ${RESET}
${FG_GRAY}      ${FG_CLAWD}${BG_CLAWD}██▄█████▄██${RESET}${FG_GRAY}                           ▒▒         ▒▒ ${RESET}
${FG_GRAY}      ${FG_CLAWD} █████████ ${RESET}${FG_GRAY}                          ░          ▒   ${RESET}
${FG_GRAY}      ${FG_CLAWD}█ █   █ █${RESET}${FG_GRAY}                                            ${RESET}
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
  console.log(`${DIM}        /help     — all commands${RESET}`);
  console.log(`${DIM}        /quit     — exit BaoClaw${RESET}`);
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
// Main
// ═══════════════════════════════════════════════════════════════
async function main() {
  const defaultBin = path.resolve(process.cwd(), 'claude-core', 'target', 'release', 'claude-core');
  const binaryPath = path.resolve(process.env.CLAUDE_CORE_BIN ?? defaultBin);

  // Check API key
  if (!process.env.ANTHROPIC_API_KEY) {
    console.error(`${FG_RED}${BOLD}Error:${RESET} ANTHROPIC_API_KEY is not set.`);
    console.error(`${DIM}Set it with: export ANTHROPIC_API_KEY=sk-ant-...${RESET}`);
    process.exit(1);
  }

  // Clear screen and print logo
  process.stdout.write(`${ESC}2J${ESC}H`);
  printLogo();

  startSpinner('Starting BaoClaw engine...');

  // Spawn Rust process
  const child: ChildProcess = spawn(binaryPath, [], {
    cwd: process.cwd(),
    stdio: ['pipe', 'pipe', 'pipe'],
    env: process.env,
  });

  let stderr = '';
  child.stderr?.on('data', (d: Buffer) => { stderr += d.toString(); });

  // Wait for SOCKET: line
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
          resolve(line.slice('SOCKET:'.length).trim());
          return;
        }
      }
    });
    child.on('error', (e) => { clearTimeout(timer); reject(e); });
    child.on('close', (code) => { clearTimeout(timer); reject(new Error(`Engine exited (code ${code})\n${stderr}`)); });
  });

  // Connect IPC
  const client = new IpcClient();
  await client.connect(socketPath);

  // Initialize
  const initResult = await client.request<{ capabilities: Record<string, unknown>; session_id: string }>(
    'initialize',
    { cwd: process.cwd(), settings: {} }
  );

  stopSpinner();
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
          process.stdout.write(`\n${FG_ORANGE}${BOLD}BaoClaw${RESET} `);
          isStreaming = true;
        }
        process.stdout.write(content);
        currentText += content;
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

      case 'result': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
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
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    prompt: `${FG_ORANGE}❯${RESET} `,
  });

  rl.prompt();

  rl.on('line', async (line: string) => {
    const input = line.trim();
    if (!input) { rl.prompt(); return; }

    if (input === '/quit' || input === '/exit' || input === '/q') {
      console.log(`\n${DIM}Shutting down...${RESET}`);
      try { await client.request('shutdown'); } catch {}
      await client.disconnect();
      child.kill();
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

    if (input === '/help') {
      console.log(`\n${FG_ORANGE}${BOLD}Commands${RESET}\n`);
      console.log(`  ${FG_WHITE}/tools${RESET}     ${DIM}List registered tools${RESET}`);
      console.log(`  ${FG_WHITE}/mcp${RESET}       ${DIM}List MCP server configurations${RESET}`);
      console.log(`  ${FG_WHITE}/skills${RESET}    ${DIM}List discovered skills${RESET}`);
      console.log(`  ${FG_WHITE}/plugins${RESET}   ${DIM}List discovered plugins${RESET}`);
      console.log(`  ${FG_WHITE}/abort${RESET}     ${DIM}Cancel current request${RESET}`);
      console.log(`  ${FG_WHITE}/clear${RESET}     ${DIM}Clear screen${RESET}`);
      console.log(`  ${FG_WHITE}/quit${RESET}      ${DIM}Exit BaoClaw${RESET}`);
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

    try {
      await client.request('submitMessage', { prompt: input });
    } catch (err) {
      stopSpinner();
      console.error(`${FG_RED}Request failed: ${err}${RESET}`);
    }

    rl.prompt();
  });

  rl.on('close', async () => {
    stopSpinner();
    console.log(`\n${DIM}Goodbye!${RESET}`);
    try { await client.request('shutdown'); } catch {}
    await client.disconnect();
    child.kill();
    process.exit(0);
  });

  // Handle Rust process crash
  child.on('close', (code) => {
    stopSpinner();
    if (code !== 0 && code !== null) {
      console.error(`\n${FG_RED}${BOLD}Engine crashed${RESET}${FG_RED} (exit code ${code})${RESET}`);
      if (stderr) console.error(`${DIM}${stderr.trim()}${RESET}`);
    }
    process.exit(code ?? 0);
  });
}

main().catch((err) => {
  stopSpinner();
  console.error(`${FG_RED}${BOLD}Fatal:${RESET} ${err.message}`);
  process.exit(1);
});
