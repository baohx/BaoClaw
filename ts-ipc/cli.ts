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
// Image helpers — save base64 images & iTerm2 inline display
// ═══════════════════════════════════════════════════════════════
const IMAGE_DIR = '/tmp/baoclaw-images';

/** Ensure the image output directory exists */
function ensureImageDir(): void {
  if (!fs.existsSync(IMAGE_DIR)) {
    fs.mkdirSync(IMAGE_DIR, { recursive: true });
  }
}

/** Save a base64-encoded image to /tmp/baoclaw-images/ and return the file path */
function saveBase64Image(base64Data: string, mediaType: string): string {
  ensureImageDir();
  const ext = mediaType.split('/')[1] || 'png';          // e.g. "png", "jpeg", "gif", "webp"
  const normalizedExt = ext === 'jpeg' ? 'jpg' : ext;
  const timestamp = Math.floor(Date.now() / 1000);
  const fileName = `baoclaw-${timestamp}.${normalizedExt}`;
  const filePath = path.join(IMAGE_DIR, fileName);
  const buffer = Buffer.from(base64Data, 'base64');
  fs.writeFileSync(filePath, buffer);
  return filePath;
}

/** Display an image inline using iTerm2 Inline Image Protocol (if supported) */
function displayIterm2Image(filePath: string): void {
  if (process.env.TERM_PROGRAM !== 'iTerm.app') return;
  try {
    const data = fs.readFileSync(filePath);
    const base64 = data.toString('base64');
    const name = path.basename(filePath);
    // iTerm2 escape: ESC ] 1337 ; File = ... BEL
    process.stdout.write(`\x1b]1337;File=inline=1;name=${name}:size=${data.length}:${base64}\x07\n`);
  } catch {
    // Silently ignore if iTerm2 display fails
  }
}

/** Extract image content blocks from a tool_result output, save & display them.
 *  Returns the number of images found. */
function extractAndSaveImages(output: unknown): number {
  if (typeof output !== 'object' || output === null) return 0;
  const o = output as Record<string, unknown>;

  let count = 0;

  // Case 1: Top-level image (ImageGenTool format)
  // { type: "image", source: { type: "base64", media_type: "...", data: "..." } }
  if (o.type === 'image' && typeof o.source === 'object' && o.source !== null) {
    const src = o.source as Record<string, unknown>;
    if (src.type === 'base64' && typeof src.data === 'string' && (src.data as string).length > 100) {
      const mediaType = typeof src.media_type === 'string' ? src.media_type : 'image/png';
      const filePath = saveBase64Image(src.data as string, mediaType);
      count++;
      const prompt = typeof o.prompt === 'string' ? ` (${(o.prompt as string).slice(0, 50)})` : '';
      console.log(`${turnPrefix()}  📷 图片已保存: ${FG_CYAN}${filePath}${RESET}${prompt}`);
      displayIterm2Image(filePath);
      return count;
    }
  }

  // Case 2: Top-level MCP image { type: "image", data: "base64...", mimeType: "..." }
  if (o.type === 'image' && typeof o.data === 'string' && (o.data as string).length > 100) {
    const mediaType = typeof o.mimeType === 'string' ? o.mimeType : 'image/png';
    const filePath = saveBase64Image(o.data as string, mediaType);
    count++;
    console.log(`${turnPrefix()}  📷 图片已保存: ${FG_CYAN}${filePath}${RESET}`);
    displayIterm2Image(filePath);
    return count;
  }

  // Case 3: Content array format (MCP/Anthropic)
  // { content: [{ type: "image", source: { type: "base64", data: "..." } }] }
  const contentArrays: unknown[][] = [];
  if (Array.isArray(o.content)) contentArrays.push(o.content);
  if (Array.isArray(output)) contentArrays.push(output as unknown[]);

  for (const arr of contentArrays) {
    for (const block of arr) {
      if (typeof block !== 'object' || block === null) continue;
      const b = block as Record<string, unknown>;
      if (b.type !== 'image') continue;

      // Anthropic format: source.data
      const src = b.source as Record<string, unknown> | undefined;
      if (src && src.type === 'base64' && typeof src.data === 'string') {
        const mediaType = typeof src.media_type === 'string' ? src.media_type : 'image/png';
        const filePath = saveBase64Image(src.data as string, mediaType);
        count++;
        console.log(`${turnPrefix()}  📷 图片已保存: ${FG_CYAN}${filePath}${RESET}`);
        displayIterm2Image(filePath);
        continue;
      }

      // MCP format: data at top level of block
      if (typeof b.data === 'string' && (b.data as string).length > 100) {
        const mediaType = typeof b.mimeType === 'string' ? b.mimeType : (typeof b.media_type === 'string' ? b.media_type : 'image/png');
        const filePath = saveBase64Image(b.data as string, mediaType);
        count++;
        console.log(`${turnPrefix()}  📷 图片已保存: ${FG_CYAN}${filePath}${RESET}`);
        displayIterm2Image(filePath);
      }
    }
  }
  return count;
}

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
    // Clear the spinner line with spaces, then advance to next line
    // so subsequent output doesn't overlap on the same line
    process.stderr.write('\r' + ' '.repeat(60) + '\r\n');
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

  console.log(`${FG_ORANGE}${BOLD}  Welcome to BaoClaw ${RESET}${DIM}v2.1.0${RESET}`);
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

  // ── FileEdit: git diff side-by-side ──
  if (toolName === 'FileEdit' || toolName === 'Edit') {
    if (isError && typeof o.error === 'string') return `  ${prefix} ${FG_RED}${o.error}${RESET}`;
    const filePath = String(o.file_path ?? '');
    const oldStr = String(o.old_string ?? '');
    const newStr = String(o.new_string ?? '');
    if (!oldStr && !newStr) return `  ${prefix} ${DIM}${filePath}${RESET}`;

    // ── Side-by-side diff rendering ──
    const oldLines = oldStr.split('\n');
    const newLines = newStr.split('\n');
    const maxLines = Math.max(oldLines.length, newLines.length);
    const halfWidth = Math.min(55, Math.floor((process.stdout.columns || 120) / 2) - 3);
    const showLines = Math.min(maxLines, 20); // cap display

    let diffLines: string[] = [];
    diffLines.push(`  ${DIM}${filePath}${RESET}`);

    // Header
    const leftHeader = `${FG_RED}─ removed (old)${RESET}`;
    const rightHeader = `${FG_GREEN}─ added (new)${RESET}`;
    diffLines.push(`  ${leftHeader.padEnd(halfWidth + 10)} │ ${rightHeader}`);
    diffLines.push(`  ${'─'.repeat(halfWidth)}─┼─${'─'.repeat(halfWidth)}`);

    for (let i = 0; i < showLines; i++) {
      const oLine = i < oldLines.length ? oldLines[i] : '';
      const nLine = i < newLines.length ? newLines[i] : '';

      const oPad = oLine.length > halfWidth ? oLine.slice(0, halfWidth - 1) + '…' : oLine;
      const nPad = nLine.length > halfWidth ? nLine.slice(0, halfWidth - 1) + '…' : nLine;

      const hasOld = i < oldLines.length;
      const hasNew = i < newLines.length;

      // Color: red for removed, green for added, white for unchanged context
      let isChanged = oLine !== nLine;
      let leftColor = !hasOld ? DIM : isChanged ? FG_RED : DIM;
      let rightColor = !hasNew ? DIM : isChanged ? FG_GREEN : DIM;
      let leftMarker = !hasOld ? ' ' : isChanged ? '-' : ' ';
      let rightMarker = !hasNew ? ' ' : isChanged ? '+' : ' ';

      const left = `${leftColor}${leftMarker} ${oPad.padEnd(halfWidth)}${RESET}`;
      const right = `${rightColor}${rightMarker} ${nPad.padEnd(halfWidth)}${RESET}`;
      diffLines.push(`  ${left} │ ${right}`);
    }

    if (maxLines > showLines) {
      diffLines.push(`  ${DIM}  … (${maxLines - showLines} more lines)${RESET}`);
    }

    // Stats
    const removed = oldLines.filter((l, i) => i >= newLines.length || l !== newLines[i]).length;
    const added = newLines.filter((l, i) => i >= oldLines.length || l !== oldLines[i]).length;
    diffLines.push(`  ${FG_RED}-${removed}${RESET}  ${FG_GREEN}+${added}${RESET}`);

    return diffLines.join('\n');
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
// Shared history display (used by auto-load and /history command)
// ═══════════════════════════════════════════════════════════════
async function showHistory(client: IpcClient, count: number) {
  try {
    const result = await client.request<{ messages: any[]; count: number; total: number }>('talkTail', { count });
    if (result.count === 0) return;
    console.log(`\n${FG_ORANGE}${BOLD}━━━ History ━━━${RESET} ${DIM}(${result.count} of ${result.total} messages)${RESET}\n`);

    // Track turn numbers for display
    for (let i = 0; i < result.messages.length; i++) {
      const m = result.messages[i];
      // Skip pure tool-result user messages (they're shown inline under assistant tools)
      if (m.role === 'user' && m.is_tool_result) continue;

      const ts = m.timestamp ? `${DIM}${m.timestamp.slice(11, 19)}${RESET}` : '';
      const turnLabel = m.turn ? `${DIM}#${m.turn}${RESET} ` : '';

      if (m.role === 'user') {
        const text = m.text || '';
        console.log(`${turnLabel}${ts} ${FG_BRIGHT_WHITE}${BOLD}You${RESET}`);
        // Show full text, indented, wrapped at terminal width
        const lines = text.split('\n');
        for (const line of lines) {
          if (line.trim()) {
            console.log(`    ${FG_WHITE}${line}${RESET}`);
          }
        }
      } else if (m.role === 'assistant') {
        const text = m.text || '';
        const cost = m.cost_usd ? `$${Number(m.cost_usd).toFixed(4)}` : '';
        const dur = m.duration_ms ? `${(m.duration_ms / 1000).toFixed(1)}s` : '';
        const usage = m.usage;
        const tokenInfo = usage
          ? `${FG_CYAN}${usage.input_tokens || 0}in/${usage.output_tokens || 0}out${usage.cache_read_input_tokens ? ` (${usage.cache_read_input_tokens}cache)` : ''}${RESET}`
          : '';
        const stats = [cost, dur, tokenInfo].filter(Boolean).join(' · ');

        const toolBadge = m.tools && m.tools.length > 0
          ? ` ${FG_MAGENTA}[${m.tools.length} tool${m.tools.length > 1 ? 's' : ''}]${RESET}`
          : '';

        console.log(`${turnLabel}${ts} ${FG_ORANGE}${BOLD}BC${RESET}${toolBadge} ${stats ? `${DIM}${stats}${RESET}` : ''}`);

        // Show full assistant text, indented
        if (text.trim()) {
          const lines = text.split('\n');
          for (const line of lines) {
            console.log(`    ${line}`);
          }
        }

        // Show tool call details with results
        if (m.tools && m.tools.length > 0) {
          for (const t of m.tools) {
            const toolName = t.name || '';
            const detail = t.detail || '';
            console.log(`    ${FG_MAGENTA}├─ ${toolName}${RESET}${detail ? ` ${DIM}${detail}${RESET}` : ''}`);
            // Show tool result if available (truncated to ~500 chars for readability)
            if (t.result) {
              const resultStr = typeof t.result === 'string' ? t.result : JSON.stringify(t.result, null, 2);
              const resultLines = resultStr.split('\n').slice(0, 15); // Max 15 lines
              for (const rl of resultLines) {
                console.log(`    ${FG_GRAY}│  ${rl.slice(0, 120)}${RESET}`);
              }
              if (resultStr.split('\n').length > 15 || resultStr.length > 1800) {
                console.log(`    ${FG_GRAY}│  ... (${resultStr.length} chars total)${RESET}`);
              }
            }
          }
        }
      } else if (m.role === 'system') {
        console.log(`  ${ts} ${DIM}[system]${RESET}`);
      }

      // Add separator between turns
      if (i < result.messages.length - 1) {
        console.log();
      }
    }
    console.log(`\n${DIM}${'─'.repeat(50)}${RESET}\n`);
  } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
}

// ═══════════════════════════════════════════════════════════════
// Daemon launcher
// ═══════════════════════════════════════════════════════════════
async function startNewDaemon(binaryPath: string, sandboxMode?: string): Promise<string> {
  startSpinner('Starting BaoClaw engine...');

  // Build daemon args: always include --daemon and --cwd; forward --sandbox if set
  const daemonArgs: string[] = ['--daemon', '--cwd', process.cwd()];
  if (sandboxMode) {
    daemonArgs.push('--sandbox', sandboxMode);
  }

  const child = spawn(binaryPath, daemonArgs, {
    cwd: process.cwd(),
    stdio: ['ignore', 'pipe', 'pipe'],
    env: process.env,
    detached: true,  // Survives parent exit
  });

  // Don't let the child keep the parent alive
  child.unref();

  // Prevent zombie: ensure daemon child is reaped even after we unref'd.
  // Without this, if the daemon crashes, its zombie persists because
  // we removed all listeners and the parent never waitpid()'s it.
  child.on('exit', () => {});

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
  '/clear', '/abort', '/task', '/voice', '/telemetry', '/telegram', '/memory', '/debug',
  '/projects', '/cron', '/history', '/doc', '/team',
  '/template', '/permission',
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
  // --debug flag: enable timing instrumentation for the first query
  const cliDebugMode = args.includes('--debug');

  // --sandbox flag: forward to daemon for sandboxed bash execution
  let sandboxMode: string | undefined;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--sandbox') {
      if (i + 1 < args.length && !args[i + 1]?.startsWith('--')) {
        sandboxMode = args[i + 1]; // e.g. --sandbox docker
        i++;
      } else {
        sandboxMode = 'auto'; // just --sandbox → auto-detect
      }
    } else if (args[i]?.startsWith('--sandbox=')) {
      sandboxMode = args[i].split('=')[1]; // e.g. --sandbox=bwrap
    }
  }
  if (sandboxMode === 'auto') {
    sandboxMode = undefined; // let baoclaw-core auto-detect (no value = just --sandbox)
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
    socketPath = await startNewDaemon(binaryPath, sandboxMode);
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

  // ── Auto-display recent history (last 5 messages) if session has history ──
  await showHistory(client, 5);

  // ── Stream event handling ──
  let isStreaming = false;
  let currentText = '';
  let toolCount = 0;
  let queryStartTime = 0;
  // Track tool_use_id → tool_name for smart result formatting
  const pendingTools = new Map<string, { name: string; input: unknown }>();

  // ── Pending attachments from /doc command ──
  let pendingAttachments: Array<Record<string, unknown>> = [];

  // ── Debug timing mode ──
  let debugMode = cliDebugMode;
  let firstQueryDone = false; // only instrument the very first query per session
  // Sub-step timestamps for the current query
  let debugSubmitTime = 0;        // when submitMessage was sent
  let debugFirstEventTime = 0;    // TTFB: first stream event (thinking_chunk / assistant_chunk)
  let debugThinkingStartTime = 0; // when first thinking_chunk arrived
  let debugThinkingEndTime = 0;   // when first assistant_chunk arrived (or tool_use if no assistant_chunk)
  let debugToolTimes = new Map<string, { name: string; start: number; end: number }>();

  function resetDebugTimers() {
    debugSubmitTime = 0;
    debugFirstEventTime = 0;
    debugThinkingStartTime = 0;
    debugThinkingEndTime = 0;
    debugToolTimes.clear();
  }

  function fmtMs(ms: number): string {
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
  }

  // ── Turn stack for nested rendering ──
  type TurnInfo = { id: number; parent: number | null; label: string | null; start: number };
  const turnStack: TurnInfo[] = [];
  function turnDepth(): number { return turnStack.length; }
  function turnPrefix(): string {
    if (turnStack.length === 0) return '';
    return turnStack.map(() => '│ ').join('');
  }
  function formatTokens(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return String(n);
  }

  // ── Session-cumulative token/cost tracking ──
  let cumulativeInputTokens = 0;
  let cumulativeOutputTokens = 0;
  let cumulativeCostUsd = 0;
  const CONTEXT_WINDOW = 200_000;

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
        // Debug: record TTFB and end of thinking phase
        if (debugMode && !firstQueryDone) {
          const now = Date.now();
          if (!debugFirstEventTime) debugFirstEventTime = now;
          if (debugThinkingStartTime > 0 && !debugThinkingEndTime) {
            debugThinkingEndTime = now;
          }
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
        // Debug: record TTFB and thinking start
        if (debugMode && !firstQueryDone) {
          const now = Date.now();
          if (!debugFirstEventTime) debugFirstEventTime = now;
          if (!debugThinkingStartTime) debugThinkingStartTime = now;
        }
        process.stdout.write(`${FG_GRAY}${content}${RESET}`);
        break;
      }

      case 'tool_use': {
        stopSpinner();
        if (isStreaming) {
          // Debug: if thinking was ongoing, mark end of thinking at tool_use
          if (debugMode && !firstQueryDone && debugThinkingStartTime > 0 && !debugThinkingEndTime) {
            debugThinkingEndTime = Date.now();
          }
          // Flush accumulated text before showing tool use
          if (currentText.trim()) {
            process.stdout.write(`\n${turnPrefix()}${FG_ORANGE}${BOLD}BaoClaw${RESET}\n`);
            const renderedLines = renderMarkdown(currentText).split('\n');
            process.stdout.write(renderedLines.map(l => turnPrefix() + l).join('\n'));
            process.stdout.write('\n');
            currentText = '';
          }
          isStreaming = false;
        }
        toolCount++;
        const tu = event as { tool_name: string; input: unknown; tool_use_id: string };
        pendingTools.set(tu.tool_use_id, { name: tu.tool_name, input: tu.input });
        // Debug: record tool start time
        if (debugMode && !firstQueryDone) {
          debugToolTimes.set(tu.tool_use_id, { name: tu.tool_name, start: Date.now(), end: 0 });
        }
        console.log(turnPrefix() + formatToolUse(tu.tool_name, tu.input));
        startSpinner(`${tu.tool_name}…`);
        break;
      }

      case 'tool_result': {
        stopSpinner();
        const tr = event as { tool_use_id: string; output: unknown; is_error: boolean };
        // Debug: record tool end time
        if (debugMode && !firstQueryDone && debugToolTimes.has(tr.tool_use_id)) {
          const entry = debugToolTimes.get(tr.tool_use_id)!;
          entry.end = Date.now();
        }
        const toolInfo = pendingTools.get(tr.tool_use_id);
        pendingTools.delete(tr.tool_use_id);
        // Extract & save any image content blocks from tool output
        extractAndSaveImages(tr.output);
        const logLevel = (globalThis as any).__baoclaw_log_level ?? 'verbose';
        // quiet: skip all tool results; normal: skip success results
        if (logLevel === 'quiet') break;
        if (logLevel === 'normal' && !tr.is_error) break;
        console.log(turnPrefix() + formatToolResult(tr.output, tr.is_error, toolInfo?.name, toolInfo?.input));
        break;
      }

      case 'turn_start': {
        const t = event as { turn_id: number; parent_turn_id: number | null; agent_label: string | null };
        turnStack.push({ id: t.turn_id, parent: t.parent_turn_id ?? null, label: t.agent_label ?? null, start: Date.now() });
        const depthBar = turnStack.length > 1 ? turnStack.slice(0, -1).map(() => '│ ').join('') : '';
        const which = t.parent_turn_id != null ? `Subagent Turn ${t.turn_id}` : `Turn ${t.turn_id}`;
        const labelText = t.agent_label ? ` ${FG_GRAY}${t.agent_label}${RESET}` : '';
        console.log(`${depthBar}${FG_ORANGE}┌─ ${which}${labelText}${RESET}`);
        break;
      }

      case 'turn_end': {
        const t = event as { turn_id: number; duration_ms: number; tool_count: number; input_tokens: number; output_tokens: number };
        const info = turnStack.pop();
        const depthBar = turnStack.map(() => '│ ').join('');
        const seconds = (t.duration_ms / 1000).toFixed(1);
        const totalTok = t.input_tokens + t.output_tokens;
        cumulativeInputTokens += t.input_tokens;
        cumulativeOutputTokens += t.output_tokens;
        console.log(
          `${depthBar}${FG_ORANGE}└─ Turn ${t.turn_id} done${RESET} ${DIM}${t.tool_count} tools, ${seconds}s, ${formatTokens(totalTok)} tokens${RESET}`
        );
        break;
      }

      case 'progress': {
        const pg = event as { tool_use_id: string; data: Record<string, unknown> };
        const msg = String(pg.data?.message ?? '');
        // Highlight compaction events prominently
        if (msg.toLowerCase().includes('compact')) {
          stopSpinner();
          console.log('');
          console.log(`${FG_CYAN}━━━━━ 📦 ${msg} ━━━━━${RESET}`);
          console.log('');
          break;
        }
        const info = pg.data?.sub_agent_tool || pg.data?.percent || msg || '';
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

          // Token/cost status footer
          if (result.total_cost_usd && result.total_cost_usd > 0) {
            cumulativeCostUsd = result.total_cost_usd;
          }
          const pct = ((cumulativeInputTokens / CONTEXT_WINDOW) * 100).toFixed(0);
          const costStr = cumulativeCostUsd > 0 ? `  💰 $${cumulativeCostUsd.toFixed(4)}` : '';
          if (cumulativeInputTokens > 0) {
            console.log(`${DIM}┃ 🔤 ${formatTokens(cumulativeInputTokens)} / ${formatTokens(CONTEXT_WINDOW)} (${pct}%)${costStr}${RESET}`);
          }
        }
        // ── Debug timing report (first query only) ──
        if (debugMode && !firstQueryDone && debugSubmitTime > 0) {
          firstQueryDone = true;
          const now = Date.now();
          const totalWall = now - debugSubmitTime;
          const ttfb = debugFirstEventTime ? fmtMs(debugFirstEventTime - debugSubmitTime) : 'n/a';
          const thinkingDur = (debugThinkingStartTime > 0 && debugThinkingEndTime > 0)
            ? fmtMs(debugThinkingEndTime - debugThinkingStartTime) : 'n/a';
          const firstTokenLatency = debugFirstEventTime ? fmtMs(debugFirstEventTime - debugSubmitTime) : 'n/a';

          let toolBreakdown = '';
          let toolTotal = 0;
          for (const [id, t] of debugToolTimes) {
            if (t.end > 0) {
              const dur = t.end - t.start;
              toolTotal += dur;
              toolBreakdown += `\n    ${FG_WHITE}${t.name}${RESET}  ${FG_CYAN}${fmtMs(dur)}${RESET}`;
            }
          }

          console.log(`\n  ${FG_YELLOW}${BOLD}⏱ Debug Timing (first query)${RESET}`);
          console.log(`    ${FG_WHITE}Total wall time:${RESET}    ${FG_CYAN}${fmtMs(totalWall)}${RESET}`);
          console.log(`    ${FG_WHITE}TTFB (first byte):${RESET}  ${FG_CYAN}${ttfb}${RESET}`);
          if (debugThinkingStartTime > 0) {
            console.log(`    ${FG_WHITE}Thinking:${RESET}           ${FG_CYAN}${thinkingDur}${RESET}`);
          }
          if (debugThinkingEndTime > 0) {
            const genStart = debugThinkingEndTime;
            const genDur = now - genStart;
            console.log(`    ${FG_WHITE}Generation:${RESET}         ${FG_CYAN}${fmtMs(genDur)}${RESET}`);
          }
          if (toolBreakdown) {
            console.log(`    ${FG_WHITE}Tools total:${RESET}        ${FG_CYAN}${fmtMs(toolTotal)}${RESET}`);
            console.log(toolBreakdown);
          }
          console.log('');
          resetDebugTimers();
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
        console.log('');
        console.log(`${FG_YELLOW}🔀 Model fallback: ${fb.from_model} → ${fb.to_model}${RESET}`);
        console.log('');
        startSpinner(fb.to_model + '…');
        break;
      }

      case 'error': {
        stopSpinner();
        if (isStreaming) { process.stdout.write('\n'); isStreaming = false; }
        const err = event as { code: string; message: string };
        console.log(`\n  ${FG_RED}✗ ${BOLD}${err.code || 'Error'}${RESET}${FG_RED}: ${err.message}${RESET}\n`);
        // Fully reset all streaming state to prevent stale echo on next input
        currentText = '';
        toolCount = 0;
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
      case 'state_update': {
        // Track context token usage for display
        const patch = (event as { patch: Record<string, unknown> }).patch;
        if (patch?.usage) {
          const u = patch.usage as { input_tokens?: number; output_tokens?: number };
          const total = (u.input_tokens || 0) + (u.output_tokens || 0);
          if (total > 600000) {
            console.log(`\n${FG_YELLOW}⚠ Context: ${(total/1000).toFixed(0)}k tokens — consider /compact${RESET}`);
          }
        }
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
  // Track whether we are inside handleLine to allow SIGINT to break out
  let abortRequested = false;
  rl.on('SIGINT', async () => {
    if (queryStartTime > 0) {
      // Task in progress — reset ALL state immediately, fire-and-forget abort
      stopSpinner();
      // Clear any partial streaming output on the current line
      readline.clearLine(process.stdout, 0);
      process.stdout.write('\r');
      console.log(`${FG_YELLOW}⚠ Aborted${RESET}\n`);
      // Fire-and-forget: don't await the abort RPC (it may hang if daemon is stuck)
      client.request('abort').catch(() => {});
      // Reset state immediately — don't wait for daemon's result event
      currentText = '';
      isStreaming = false;
      toolCount = 0;
      queryStartTime = 0;
      processingInput = false;
      abortRequested = true;
      ctrlCCount = 0;
      rl.prompt();
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
      // Flush any remaining text the user is still editing on the readline prompt line.
      // When pasting multi-line content, the last line often stays in readline's
      // internal buffer without triggering a 'line' event (no trailing \n).
      const pendingLine = (rl as any).line;
      if (typeof pendingLine === 'string' && pendingLine.length > 0) {
        pasteBuffer.push(pendingLine);
        // Clear readline's internal buffer and refresh the prompt line
        (rl as any).line = '';
        (rl as any).cursor = 0;
        readline.moveCursor(process.stdout, 0, 0);
        readline.clearLine(process.stdout, 0);
      }

      const lines = pasteBuffer;
      pasteBuffer = [];
      pasteTimer = null;

      // If single line, process normally
      if (lines.length === 1) {
        const input = lines[0].trim();
        if (!input) { rl.prompt(); return; }
        // Clear readline's native echo to avoid double-display
        readline.moveCursor(process.stdout, 0, -1);
        readline.clearLine(process.stdout, 0);
        process.stdout.write('\r');
        await handleInput(input);
        return;
      }

      // Multi-line paste — clear readline's native echo of the first line to avoid
      // double-display (readline already echoed it, and handleLine will print "You …" again)
      readline.moveCursor(process.stdout, 0, -(lines.length > 1 ? 1 : 0));
      for (let i = 0; i < lines.length; i++) {
        readline.clearLine(process.stdout, 0);
        readline.moveCursor(process.stdout, 0, -1);
      }
      readline.moveCursor(process.stdout, 0, 1);
      process.stdout.write('\r');

      const combined = lines.join('\n').trim();
      if (!combined) { rl.prompt(); return; }

      // ── Threshold for summarizing paste (≥5 lines or ≥2KB) ──
      const PASTE_LINE_THRESHOLD = 5;
      const PASTE_SIZE_THRESHOLD = 2048;

      if (lines.length >= PASTE_LINE_THRESHOLD || combined.length >= PASTE_SIZE_THRESHOLD) {
        const totalLines = lines.length;
        const totalBytes = Buffer.byteLength(combined, 'utf-8');
        const sizeStr = totalBytes >= 1024 ? `${(totalBytes / 1024).toFixed(1)}KB` : `${totalBytes}B`;

        // Detect content type for better summary
        const firstLine = lines[0].trim();
        let contentType = 'text';
        if (firstLine.startsWith('{') || firstLine.startsWith('[')) contentType = 'JSON';
        else if (firstLine.startsWith('<') || firstLine.startsWith('<?xml') || firstLine.startsWith('<!DOCTYPE')) contentType = 'XML/HTML';
        else if (firstLine.startsWith('#!') || firstLine.startsWith('import ') || firstLine.startsWith('use ') || firstLine.startsWith('fn ') || firstLine.startsWith('function ') || firstLine.startsWith('const ') || firstLine.startsWith('pub ')) contentType = 'code';
        else if (firstLine.startsWith('diff --git') || firstLine.startsWith('---') || firstLine.startsWith('+++')) contentType = 'git diff';
        else if (firstLine.startsWith('commit ') || firstLine.startsWith('Author:')) contentType = 'git log';
        else if (lines.some(l => l.trim().startsWith('error') || l.trim().startsWith('Error') || l.trim().startsWith('panic'))) contentType = 'error log';
        else if (lines.some(l => /^\s*\d{4}-\d{2}-\d{2}/.test(l.trim()))) contentType = 'log';

        const head = lines.slice(0, 2).join('\n');
        const tail = lines.slice(-1).join('\n');
        const headPreview = head.length > 80 ? head.slice(0, 80) + '…' : head;
        const tailPreview = totalLines > 3 && tail.length > 60 ? '…\n' + tail.slice(0, 60) + '…' : '';

        // Show paste summary
        console.log('');
        console.log(`  ${FG_YELLOW}📋 Pasted ${totalLines} lines (${sizeStr}) of ${contentType}${RESET}`);
        if (headPreview) console.log(`  ${DIM}${headPreview}${RESET}`);
        if (tailPreview) console.log(`  ${DIM}${tailPreview}${RESET}`);
        console.log(`  ${DIM}─── Enter additional instructions (or press Enter to send as-is) ───${RESET}`);

        // Pause readline, let user type additional instructions
        // Use a separate interface with terminal: false to avoid echo conflicts
        rl.pause();
        const { createInterface } = await import('readline');
        const mlRl = createInterface({ 
          input: process.stdin, 
          output: process.stdout,
          terminal: false,  // Disable terminal mode to avoid echo conflicts
        });
        const extraInput: string = await new Promise(resolve => {
          process.stdout.write(`  ${FG_CYAN}➤${RESET} `);
          mlRl.once('line', (answer: string) => {
            mlRl.close();
            resolve(answer.trim());
          });
        });
        rl.resume();

        // Build final message: summary header + full content + extra instructions
        let finalMessage = `[User pasted ${totalLines} lines (${sizeStr}) of ${contentType}]\n\n${combined}`;
        if (extraInput) {
          finalMessage += `\n\n[User's additional instruction: ${extraInput}]`;
          console.log(`  ${DIM}✓ Appended instruction: "${extraInput.slice(0, 80)}${extraInput.length > 80 ? '…' : ''}"${RESET}`);
        } else {
          console.log(`  ${DIM}✓ Sending paste content as-is${RESET}`);
        }
        console.log('');
        await handleInput(finalMessage);
      } else {
        // Short paste: just join and send
        await handleInput(combined);
      }
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
      // Get daemon PID from the .json metadata next to the socket
      let daemonPid: number | null = null;
      try {
        const socketDir = path.join(os.tmpdir(), 'baoclaw-sockets');
        for (const file of fs.readdirSync(socketDir)) {
          if (!file.endsWith('.json')) continue;
          try {
            const meta = JSON.parse(fs.readFileSync(path.join(socketDir, file), 'utf-8'));
            if (meta.socket === socketPath) { daemonPid = meta.pid; break; }
          } catch {}
        }
      } catch {}
      try { await client.request('shutdown'); } catch {}
      await client.disconnect();
      // Wait for daemon to exit gracefully, then force-kill if needed
      if (daemonPid) {
        const deadline = Date.now() + 3000;
        while (Date.now() < deadline) {
          try { process.kill(daemonPid, 0); } catch { break; } // process gone
          await new Promise(r => setTimeout(r, 200));
        }
        try { process.kill(daemonPid, 0); // still alive?
          process.kill(daemonPid, 'SIGKILL');
        } catch {}
      }
      process.exit(0);
    }

    if (input === '/abort') {
      stopSpinner();
      try { await client.request('abort'); } catch {}
      currentText = '';
      isStreaming = false;
      toolCount = 0;
      queryStartTime = 0;
      console.log(`${FG_YELLOW}⚠ Aborted.${RESET}`);
      rl.prompt();
      return;
    }

    if (input.startsWith('/verbose')) {
      const arg = input.slice('/verbose'.length).trim();
      type LogLevel = 'quiet' | 'normal' | 'verbose';
      const levels: LogLevel[] = ['quiet', 'normal', 'verbose'];
      if (arg === '' || arg === 'help') {
        console.log(`${DIM}Levels: quiet | normal | verbose${RESET}`);
        console.log(`${DIM}Usage: /verbose <level>${RESET}`);
      } else if (levels.includes(arg as LogLevel)) {
        (globalThis as any).__baoclaw_log_level = arg;
        console.log(`${FG_GREEN}✓ Log level: ${arg}${RESET}`);
      } else {
        console.log(`${FG_RED}Unknown level: ${arg}${RESET}`);
      }
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
      await showHistory(client, count);
      rl.prompt();
      return;
    }

    // ── /doc <filepath> — attach document for next message ──
    if (input.startsWith('/doc')) {
      const filePath = input.slice('/doc'.length).trim();
      if (!filePath) {
        console.log(`\n${FG_ORANGE}${BOLD}Usage:${RESET} /doc <filepath>`);
        console.log(`  ${DIM}Attach a PDF or DOCX file to the next message.${RESET}`);
        console.log(`  ${DIM}Supported formats: .pdf, .docx${RESET}`);
        console.log(`  ${DIM}Max file size: 10MB${RESET}\n`);
        rl.prompt();
        return;
      }
      try {
        const result = await client.request<{ attachment: Record<string, unknown>; file_path: string }>('docUpload', { file_path: filePath });
        pendingAttachments.push(result.attachment);
        const basename = filePath.split('/').pop() || filePath;
        console.log(`\n${FG_GREEN}📎 Attached:${RESET} ${basename} ${DIM}(will be sent with next message)${RESET}\n`);
      } catch (err: any) {
        const msg = err?.message || String(err);
        console.log(`\n${FG_RED}❌ ${msg}${RESET}\n`);
      }
      rl.prompt();
      return;
    }

    if (input === '/debug') {
      debugMode = !debugMode;
      if (debugMode) {
        firstQueryDone = false;
        resetDebugTimers();
      }
      console.log(`\n${debugMode ? FG_GREEN + BOLD + 'Debug timing ON' + RESET + DIM + ' (will show detailed sub-step timing for the next query)' : FG_YELLOW + 'Debug timing OFF' + RESET}\n`);
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
      } else if (subCmd === 'stats') {
        try {
          const stats = await client.request<any>('memoryStats');
          console.log(`\n${FG_ORANGE}${BOLD}Memory Statistics${RESET}\n`);
          for (const [k, v] of Object.entries(stats)) {
            console.log(`  ${FG_WHITE}${String(k).padEnd(24)}${RESET} ${FG_CYAN}${typeof v === 'number' && !Number.isInteger(v) ? (v as number).toFixed(3) : v}${RESET}`);
          }
          console.log();
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else if (subCmd === 'archive') {
        if (!rest) {
          console.log(`\n${FG_YELLOW}Usage: /memory archive <id>${RESET}\n`);
        } else {
          try {
            const result = await client.request<{ archived: any }>('memoryArchive', { id: rest });
            console.log(`\n${FG_GREEN}✓ Archived${RESET} ${DIM}[${result.archived.id}] ${result.archived.content}${RESET}\n`);
          } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        }
      } else if (subCmd === 'restore') {
        if (!rest) {
          console.log(`\n${FG_YELLOW}Usage: /memory restore <id>${RESET}\n`);
        } else {
          try {
            const result = await client.request<{ restored: any }>('memoryRestore', { id: rest });
            console.log(`\n${FG_GREEN}✓ Restored${RESET} ${DIM}[${result.restored.id}] ${result.restored.content}${RESET}\n`);
          } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        }
      } else if (subCmd === 'archives' || subCmd === 'archived') {
        try {
          const result = await client.request<{ archived: any[]; count: number }>('memoryArchiveList');
          if (!result.count) {
            console.log(`\n${DIM}No archived memories.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Archived Memories${RESET} ${DIM}(${result.count})${RESET}\n`);
            for (const m of result.archived) {
              const content = m.content.length > 80 ? m.content.slice(0, 80) + '…' : m.content;
              console.log(`  ${FG_CYAN}${(m.category || 'fact').padEnd(10)}${RESET} ${FG_WHITE}${content}${RESET}  ${DIM}[${m.id}] imp=${(m.importance ?? 0).toFixed?.(2) ?? m.importance}${RESET}`);
            }
            console.log();
          }
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else if (subCmd === 'cleanup') {
        try {
          const result = await client.request<any>('memoryCleanup');
          console.log(`\n${FG_GREEN}✓ Cleanup complete${RESET} ${DIM}archived=${result.archived_count ?? 0} deleted=${result.deleted_count ?? 0} (${result.duration_ms ?? 0}ms)${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
      } else {
        console.log(`\n${FG_ORANGE}${BOLD}Memory Commands${RESET}\n`);
        console.log(`  ${FG_WHITE}/memory list${RESET}                    ${DIM}List all memories${RESET}`);
        console.log(`  ${FG_WHITE}/memory add [category] <text>${RESET}  ${DIM}Add a memory (fact/preference/decision)${RESET}`);
        console.log(`  ${FG_WHITE}/memory delete <id>${RESET}            ${DIM}Delete a memory${RESET}`);
        console.log(`  ${FG_WHITE}/memory clear${RESET}                  ${DIM}Clear all memories${RESET}`);
        console.log(`  ${FG_WHITE}/memory stats${RESET}                  ${DIM}Memory statistics${RESET}`);
        console.log(`  ${FG_WHITE}/memory archive <id>${RESET}           ${DIM}Archive a memory${RESET}`);
        console.log(`  ${FG_WHITE}/memory restore <id>${RESET}           ${DIM}Restore an archived memory${RESET}`);
        console.log(`  ${FG_WHITE}/memory archives${RESET}               ${DIM}List archived memories${RESET}`);
        console.log(`  ${FG_WHITE}/memory cleanup${RESET}                ${DIM}Run decay & archive cleanup now${RESET}`);
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
          // Debug: record submit time
          if (debugMode && !firstQueryDone) {
            resetDebugTimers();
            debugSubmitTime = Date.now();
          }
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

    // ── /team commands ──
    if (input.startsWith('/team')) {
      const teamArgs = input.slice('/team'.length).trim();
      const parts = teamArgs.split(/\s+/);
      const subCmd = parts[0] || '';

      if (subCmd === 'spawn') {
        // Parse: /team spawn [n] --parallel|--sequence|--dag "task"
        // Examples:
        //   /team spawn 3 --parallel "Analyze codebase"
        //   /team spawn --sequence "First analyze, then implement"
        //   /team spawn --dag "Check code style and tests, then generate report"

        let count: number | undefined;
        let mode: 'parallel' | 'sequence' | 'dag' = 'parallel';
        let task: string = '';

        // Parse the remaining arguments
        const rest = teamArgs.slice('spawn'.length).trim();

        // Check for --parallel, --sequence, --dag
        if (rest.includes('--parallel')) {
          mode = 'parallel';
          // Check if there's a number before --parallel
          const numMatch = rest.match(/^(\d+)\s+--parallel/);
          if (numMatch) {
            count = parseInt(numMatch[1], 10);
          }
        } else if (rest.includes('--sequence')) {
          mode = 'sequence';
        } else if (rest.includes('--dag')) {
          mode = 'dag';
        }

        // Extract task from quotes
        const taskMatch = rest.match(/"([^"]+)"/);
        if (taskMatch) {
          task = taskMatch[1];
        }

        if (!task) {
          console.log(`\n${FG_YELLOW}Usage: /team spawn [n] --parallel|--sequence|--dag "task"${RESET}`);
          console.log(`${DIM}  /team spawn 3 --parallel "Analyze codebase"${RESET}`);
          console.log(`${DIM}  /team spawn --sequence "First analyze, then implement"${RESET}`);
          console.log(`${DIM}  /team spawn --dag "Check code style, then report"${RESET}\n`);
          rl.prompt();
          return;
        }

        startSpinner(`Creating ${mode} team...`);
        try {
          const result = await client.request<{ team_id: string; status: string }>('teamSpawn', {
            count,
            mode,
            task,
          });
          stopSpinner();
          console.log(`\n${FG_GREEN}${BOLD}Team created${RESET} ${DIM}[${result.team_id}] ${mode}${count ? ` (${count} agents)` : ''}${RESET}`);
          console.log(`${DIM}  Task: ${task}${RESET}`);
          console.log(`${DIM}  Execute with: /team exec ${result.team_id}${RESET}\n`);
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}Failed to create team: ${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'list' || subCmd === '') {
        try {
          const result = await client.request<{ teams: Array<{ id: string; task: string; mode: string; status: string; agent_count: number; created_at: string }>; count: number }>('teamList');
          if (result.count === 0) {
            console.log(`\n${DIM}No teams. Use /team spawn to create one.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Teams${RESET} ${DIM}(${result.count})${RESET}\n`);
            for (const t of result.teams) {
              const statusIcon = t.status === 'Running' ? `${FG_YELLOW}●${RESET}`
                : t.status === 'Completed' ? `${FG_GREEN}●${RESET}`
                : t.status === 'Failed' ? `${FG_RED}●${RESET}`
                : `${FG_GRAY}●${RESET}`;
              const taskPreview = t.task.length > 50 ? t.task.slice(0, 50) + '…' : t.task;
              console.log(`  ${statusIcon} ${FG_WHITE}${t.id}${RESET}  ${DIM}${t.mode}${RESET}  ${FG_WHITE}${taskPreview}${RESET}`);
              console.log(`    ${DIM}${t.agent_count} agents · ${t.status} · ${t.created_at}${RESET}`);
            }
            console.log();
          }
        } catch (err) {
          console.error(`${FG_RED}Failed to list teams: ${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'status') {
        const teamId = parts[1] || '';
        if (!teamId) {
          console.log(`\n${FG_YELLOW}Usage: /team status <id>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const t = await client.request<{ id: string; task: string; mode: string; status: string; agents: Array<{ id: string; status: string; prompt: string }>; total_cost_usd: number; total_tokens: number }>('teamStatus', { team_id: teamId });
          const statusColor = t.status === 'Running' ? FG_YELLOW
            : t.status === 'Completed' ? FG_GREEN
            : t.status === 'Failed' ? FG_RED : FG_GRAY;
          console.log(`\n${FG_ORANGE}${BOLD}Team${RESET} ${FG_WHITE}${t.id}${RESET}`);
          console.log(`  ${FG_WHITE}Status:${RESET}  ${statusColor}${t.status}${RESET}`);
          console.log(`  ${FG_WHITE}Mode:${RESET}    ${t.mode}`);
          console.log(`  ${FG_WHITE}Task:${RESET}    ${t.task}`);
          if (t.total_cost_usd > 0) {
            console.log(`  ${FG_WHITE}Cost:${RESET}    $${t.total_cost_usd.toFixed(4)}`);
          }
          if (t.total_tokens > 0) {
            console.log(`  ${FG_WHITE}Tokens:${RESET}  ${t.total_tokens.toLocaleString()}`);
          }
          if (t.agents && t.agents.length > 0) {
            console.log(`\n  ${FG_GRAY}── Agents (${t.agents.length}) ──${RESET}`);
            for (const a of t.agents) {
              const aStatusIcon = a.status === 'Running' ? `${FG_YELLOW}●${RESET}`
                : a.status === 'Completed' ? `${FG_GREEN}●${RESET}`
                : a.status === 'Failed' ? `${FG_RED}●${RESET}`
                : `${FG_GRAY}●${RESET}`;
              const promptPreview = a.prompt.length > 40 ? a.prompt.slice(0, 40) + '…' : a.prompt;
              console.log(`    ${aStatusIcon} ${DIM}${a.id}${RESET}  ${promptPreview}`);
            }
          }
          console.log();
        } catch (err) {
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'exec' || subCmd === 'execute') {
        const teamId = parts[1] || '';
        if (!teamId) {
          console.log(`\n${FG_YELLOW}Usage: /team exec <id>${RESET}\n`);
          rl.prompt();
          return;
        }
        startSpinner(`Executing team ${teamId}...`);
        try {
          const result = await client.request<{ team_id: string; success: boolean; duration_ms: number }>('teamExecute', { team_id: teamId });
          stopSpinner();
          if (result.success) {
            const dur = result.duration_ms >= 1000
              ? `${(result.duration_ms / 1000).toFixed(1)}s`
              : `${result.duration_ms}ms`;
            console.log(`\n${FG_GREEN}${BOLD}Team completed${RESET} ${DIM}${result.team_id} in ${dur}${RESET}\n`);
          } else {
            console.log(`\n${FG_RED}Team execution failed${RESET} ${DIM}${result.team_id}${RESET}\n`);
          }
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}Failed to execute team: ${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'results') {
        const teamId = parts[1] || '';
        if (!teamId) {
          console.log(`\n${FG_YELLOW}Usage: /team results <id>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const result = await client.request<{ team_id: string; results: Array<{ agent_id: string; status: string; result?: string; error?: string }> }>('teamResults', { team_id: teamId });
          console.log(`\n${FG_ORANGE}${BOLD}Team Results${RESET} ${FG_WHITE}${result.team_id}${RESET}\n`);
          for (const r of result.results) {
            const statusIcon = r.status === 'Completed' ? `${FG_GREEN}✓${RESET}`
              : r.status === 'Failed' ? `${FG_RED}✗${RESET}`
              : `${FG_GRAY}○${RESET}`;
            console.log(`  ${statusIcon} ${FG_WHITE}${r.agent_id}${RESET}`);
            if (r.result) {
              const lines = r.result.split('\n').slice(0, 5);
              for (const line of lines) {
                console.log(`    ${DIM}${line.slice(0, 100)}${RESET}`);
              }
              if (r.result.split('\n').length > 5) {
                console.log(`    ${DIM}... (${r.result.length} chars total)${RESET}`);
              }
            }
            if (r.error) {
              console.log(`    ${FG_RED}${r.error}${RESET}`);
            }
            console.log();
          }
        } catch (err) {
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'abort') {
        const teamId = parts[1] || '';
        if (!teamId) {
          console.log(`\n${FG_YELLOW}Usage: /team abort <id>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const result = await client.request<{ team_id: string; aborted: boolean }>('teamAbort', { team_id: teamId });
          if (result.aborted) {
            console.log(`\n${FG_GREEN}Team ${teamId} aborted.${RESET}\n`);
          } else {
            console.log(`\n${FG_YELLOW}Team ${teamId} was not running or not found.${RESET}\n`);
          }
        } catch (err) {
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      // Unknown /team subcommand — show help
      console.log(`\n${FG_ORANGE}${BOLD}Team Commands${RESET}\n`);
      console.log(`  ${FG_WHITE}/team spawn [n] --parallel "task"${RESET}  ${DIM}Create parallel team${RESET}`);
      console.log(`  ${FG_WHITE}/team spawn --sequence "task"${RESET}    ${DIM}Create sequential team${RESET}`);
      console.log(`  ${FG_WHITE}/team spawn --dag "task"${RESET}         ${DIM}Create DAG team${RESET}`);
      console.log(`  ${FG_WHITE}/team list${RESET}                       ${DIM}List all teams${RESET}`);
      console.log(`  ${FG_WHITE}/team status <id>${RESET}                 ${DIM}Show team status${RESET}`);
      console.log(`  ${FG_WHITE}/team exec <id>${RESET}                   ${DIM}Execute a team${RESET}`);
      console.log(`  ${FG_WHITE}/team results <id>${RESET}                ${DIM}Show team results${RESET}`);
      console.log(`  ${FG_WHITE}/team abort <id>${RESET}                  ${DIM}Abort a running team${RESET}`);
      console.log();
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

    // ── /template commands ──
    if (input.startsWith('/template')) {
      const tplArgs = input.slice('/template'.length).trim();
      const parts = tplArgs.split(/\s+/);
      const subCmd = parts[0] || '';

      if (subCmd === 'list' || subCmd === '') {
        try {
          const result = await client.request<{ templates: any[]; count: number }>('templateList');
          if (result.count === 0) {
            console.log(`\n${DIM}No templates found. Use /template create to create one.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Templates${RESET} ${DIM}(${result.count})${RESET}\n`);
            for (const t of result.templates) {
              const builtin = t.builtin ? ` ${FG_BLUE}[builtin]${RESET}` : '';
              const trigger = t.trigger ? ` ${DIM}trigger:${RESET} ${FG_WHITE}${t.trigger}${RESET}` : '';
              console.log(`  ${FG_WHITE}${BOLD}${t.name}${RESET}${builtin}`);
              console.log(`    ${DIM}${t.description || 'No description'}${RESET}  v${t.version}${trigger}  ${DIM}steps:${t.steps_count} vars:${t.variables_count}${RESET}`);
            }
            console.log();
          }
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (subCmd === 'create') {
        const rest = tplArgs.slice('create'.length).trim();
        const match = rest.match(/^(\S+)\s+(\S+)\s+(.+)/);
        if (!match) {
          console.log(`\n${FG_YELLOW}Usage: /template create <name> <trigger> <description>${RESET}\n`);
          rl.prompt();
          return;
        }
        const name = match[1];
        const trigger = match[2];
        const description = match[3];
        const templateJson = JSON.stringify({ name, trigger, description, version: '1.0.0', workflow: [], variables: {} });
        try {
          await client.request('templateCreate', { json: templateJson });
          console.log(`\n${FG_GREEN}✓ Template created:${RESET} ${FG_WHITE}${name}${RESET} ${DIM}${trigger}${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (subCmd === 'delete' || subCmd === 'rm') {
        const name = parts.slice(1).join(' ');
        if (!name) {
          console.log(`\n${FG_YELLOW}Usage: /template delete <name>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          await client.request('templateDelete', { name });
          console.log(`\n${FG_GREEN}✓ Template deleted:${RESET} ${FG_WHITE}${name}${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (subCmd === 'export') {
        const name = parts.slice(1).join(' ');
        if (!name) {
          console.log(`\n${FG_YELLOW}Usage: /template export <name>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const result = await client.request<{ name: string; template: any }>('templateExport', { name });
          console.log(`\n${FG_ORANGE}${BOLD}Template: ${result.name}${RESET}\n`);
          console.log(JSON.stringify(result.template, null, 2));
          console.log();
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (subCmd === 'import') {
        const url = parts.slice(1).join(' ');
        if (!url) {
          console.log(`\n${FG_YELLOW}Usage: /template import <url>${RESET}\n`);
          rl.prompt();
          return;
        }
        startSpinner('Importing template...');
        try {
          const result = await client.request<{ success: boolean; name: string; trigger: string }>('templateImport', { url });
          stopSpinner();
          console.log(`\n${FG_GREEN}✓ Template imported:${RESET} ${FG_WHITE}${result.name}${RESET} ${DIM}${result.trigger}${RESET}\n`);
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      console.log(`\n${FG_ORANGE}${BOLD}Template Commands${RESET}\n`);
      console.log(`  ${FG_WHITE}/template list${RESET}                    ${DIM}List all templates${RESET}`);
      console.log(`  ${FG_WHITE}/template create <name> <t> <d>${RESET}    ${DIM}Create a template${RESET}`);
      console.log(`  ${FG_WHITE}/template delete <name>${RESET}             ${DIM}Delete a template${RESET}`);
      console.log(`  ${FG_WHITE}/template export <name>${RESET}             ${DIM}Export template as JSON${RESET}`);
      console.log(`  ${FG_WHITE}/template import <url>${RESET}              ${DIM}Import template from URL${RESET}\n`);
      rl.prompt();
      return;
    }

    // ── Extended /git commands ──
    if (input.startsWith('/git ')) {
      const gitArgs = input.slice('/git'.length).trim();
      const parts = gitArgs.split(/\s+/);
      const subCmd = parts[0] || '';

      if (subCmd === 'pr' && parts[1] === 'list') {
        startSpinner('Fetching pull requests...');
        try {
          const result = await client.request<{ pull_requests?: any[]; error?: string }>('gitPrList');
          stopSpinner();
          if (result.error) {
            console.log(`\n${FG_YELLOW}${result.error}${RESET}\n`);
          } else if (!result.pull_requests || result.pull_requests.length === 0) {
            console.log(`\n${DIM}No open pull requests.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Pull Requests${RESET} ${DIM}(${result.pull_requests.length})${RESET}\n`);
            for (const pr of result.pull_requests) {
              console.log(`  ${FG_CYAN}#${pr.number}${RESET} ${FG_WHITE}${pr.title}${RESET}`);
              console.log(`    ${DIM}${pr.state}${RESET}  ${pr.head_branch} → ${pr.base_branch}  ${DIM}by ${pr.author}${RESET}  ${pr.url}`);
            }
            console.log();
          }
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'pr' && parts[1] === 'create') {
        const title = parts.slice(2).join(' ');
        if (!title) {
          console.log(`\n${FG_YELLOW}Usage: /git pr create <title>${RESET}\n`);
          rl.prompt();
          return;
        }
        startSpinner('Creating PR...');
        try {
          const result = await client.request<{ success: boolean; number?: number; url?: string; error?: string }>('gitPrCreate', { title, body: '', base: '', head: '' });
          stopSpinner();
          if (result.success) {
            console.log(`\n${FG_GREEN}✓ PR #${result.number} created${RESET} ${DIM}${result.url}${RESET}\n`);
          } else {
            console.log(`\n${FG_RED}${result.error}${RESET}\n`);
          }
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'branch') {
        startSpinner('Listing branches...');
        try {
          const result = await client.request<{ branches?: any[]; error?: string }>('gitBranchList');
          stopSpinner();
          if (result.error) {
            console.log(`\n${FG_YELLOW}${result.error}${RESET}\n`);
          } else if (!result.branches || result.branches.length === 0) {
            console.log(`\n${DIM}No branches.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Branches${RESET} ${DIM}(${result.branches.length})${RESET}\n`);
            for (const b of result.branches) {
              const marker = b.is_current ? `${FG_GREEN}*${RESET}` : ' ';
              const ahead = b.ahead > 0 ? ` ${FG_GREEN}↑${b.ahead}${RESET}` : '';
              const behind = b.behind > 0 ? ` ${FG_RED}↓${b.behind}${RESET}` : '';
              console.log(`  ${marker} ${FG_WHITE}${b.name}${RESET}${ahead}${behind}  ${DIM}${b.last_commit} ${b.last_commit_msg}${RESET}`);
            }
            console.log();
          }
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'conflict') {
        startSpinner('Checking conflicts...');
        try {
          const result = await client.request<{ conflicts?: any[]; has_conflicts?: boolean; error?: string }>('gitConflictCheck');
          stopSpinner();
          if (result.error) {
            console.log(`\n${FG_YELLOW}${result.error}${RESET}\n`);
          } else if (!result.has_conflicts) {
            console.log(`\n${FG_GREEN}✓ No conflicts detected${RESET}\n`);
          } else {
            console.log(`\n${FG_RED}${BOLD}Conflicts detected${RESET} ${DIM}(${(result.conflicts || []).length})${RESET}\n`);
            for (const c of (result.conflicts || [])) {
              console.log(`  ${FG_YELLOW}⚠${RESET} ${FG_WHITE}${c.file}${RESET} ${c.resolved ? FG_GREEN + 'resolved' : FG_RED + 'unresolved'}${RESET}`);
            }
            console.log();
          }
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      console.log(`\n${FG_ORANGE}${BOLD}Git Commands${RESET}\n`);
      console.log(`  ${FG_WHITE}/git${RESET}               ${DIM}Git status (branch, changes)${RESET}`);
      console.log(`  ${FG_WHITE}/git pr list${RESET}       ${DIM}List pull requests${RESET}`);
      console.log(`  ${FG_WHITE}/git pr create <title>${RESET} ${DIM}Create a pull request${RESET}`);
      console.log(`  ${FG_WHITE}/git branch${RESET}         ${DIM}List branches${RESET}`);
      console.log(`  ${FG_WHITE}/git conflict${RESET}       ${DIM}Check for merge conflicts${RESET}`);
      console.log(`  ${FG_WHITE}/diff${RESET}              ${DIM}Git diff summary${RESET}`);
      console.log(`  ${FG_WHITE}/commit <msg>${RESET}      ${DIM}Stage all and commit${RESET}\n`);
      rl.prompt();
      return;
    }

    // ── Extended /model commands ──
    if (input.startsWith('/model ')) {
      const modelArgs = input.slice('/model'.length).trim();
      const parts = modelArgs.split(/\s+/);
      const subCmd = parts[0] || '';

      if (subCmd === 'list') {
        try {
          const result = await client.request<{ models: any[]; count: number }>('modelList');
          console.log(`\n${FG_ORANGE}${BOLD}Available Models${RESET} ${DIM}(${result.count})${RESET}\n`);
          for (const m of result.models) {
            const costIn = `$${m.cost_per_1k_input}/1K`;
            const costOut = `$${m.cost_per_1k_output}/1K`;
            const caps = (m.capabilities || []).join(', ');
            console.log(`  ${FG_WHITE}${BOLD}${m.name}${RESET}  ${DIM}${m.provider}${RESET}  priority:${m.priority}`);
            console.log(`    ${DIM}ctx:${(m.max_tokens/1000).toFixed(0)}K  in:${costIn}  out:${costOut}${caps ? '  [' + caps + ']' : ''}${RESET}`);
          }
          console.log();
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (subCmd === 'route') {
        const task = parts.slice(1).join(' ');
        if (!task) {
          console.log(`\n${FG_YELLOW}Usage: /model route <task description>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const result = await client.request<{ selected_model: string; reason: string; confidence: number }>('modelRoute', { task });
          console.log(`\n${FG_ORANGE}${BOLD}Model Routing${RESET}\n`);
          console.log(`  ${FG_WHITE}Task:${RESET} ${task}`);
          console.log(`  ${FG_WHITE}Model:${RESET} ${FG_GREEN}${result.selected_model}${RESET}`);
          console.log(`  ${FG_WHITE}Confidence:${RESET} ${(result.confidence * 100).toFixed(0)}%`);
          console.log(`  ${DIM}${result.reason}${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (subCmd === 'budget') {
        try {
          const result = await client.request<{
            daily_limit: number; monthly_limit: number;
            current_daily: number; current_monthly: number;
            remaining_daily: number; remaining_monthly: number;
          }>('modelBudget');
          const dailyPct = result.daily_limit > 0 ? (result.current_daily / result.daily_limit * 100).toFixed(1) : '0';
          const monthlyPct = result.monthly_limit > 0 ? (result.current_monthly / result.monthly_limit * 100).toFixed(1) : '0';
          console.log(`\n${FG_ORANGE}${BOLD}Budget${RESET}\n`);
          console.log(`  ${FG_WHITE}Daily:${RESET}   $${result.current_daily.toFixed(4)} / $${result.daily_limit.toFixed(2)} ${DIM}(${dailyPct}%)${RESET}  ${FG_GREEN}$${result.remaining_daily.toFixed(4)} remaining${RESET}`);
          console.log(`  ${FG_WHITE}Monthly:${RESET} $${result.current_monthly.toFixed(4)} / $${result.monthly_limit.toFixed(2)} ${DIM}(${monthlyPct}%)${RESET}  ${FG_GREEN}$${result.remaining_monthly.toFixed(4)} remaining${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      // /model alone (existing behavior) — show current model and allow switch
      // falls through to the existing handler below
    }

    // ── Extended /telemetry commands ──
    if (input.startsWith('/telemetry ')) {
      const telArgs = input.slice('/telemetry'.length).trim();
      const parts = telArgs.split(/\s+/);
      const subCmd = parts[0] || '';

      if (subCmd === 'stats') {
        startSpinner('Loading telemetry...');
        try {
          const result = await client.request<{
            total_turns: number; total_tokens: number; total_cost_usd: number;
            total_tools_called: number; sessions_count: number; files_modified: number;
            avg_response_time_ms: number; most_used_tool: string | null;
          }>('telemetryStats');
          stopSpinner();
          console.log(`\n${FG_ORANGE}${BOLD}Telemetry Stats${RESET}\n`);
          console.log(`  ${FG_WHITE}Turns:${RESET}       ${result.total_turns}`);
          console.log(`  ${FG_WHITE}Tokens:${RESET}      ${(result.total_tokens / 1000).toFixed(1)}K`);
          console.log(`  ${FG_WHITE}Cost:${RESET}        $${result.total_cost_usd.toFixed(4)}`);
          console.log(`  ${FG_WHITE}Tools Called:${RESET} ${result.total_tools_called}`);
          console.log(`  ${FG_WHITE}Sessions:${RESET}     ${result.sessions_count}`);
          console.log(`  ${FG_WHITE}Files Modified:${RESET} ${result.files_modified}`);
          console.log(`  ${FG_WHITE}Avg Response:${RESET}  ${result.avg_response_time_ms.toFixed(0)}ms`);
          if (result.most_used_tool) console.log(`  ${FG_WHITE}Top Tool:${RESET}     ${result.most_used_tool}`);
          console.log();
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'trends') {
        const days = parseInt(parts[1], 10) || 7;
        startSpinner(`Loading ${days}-day trends...`);
        try {
          const result = await client.request<{ days: number; daily: any[]; count: number }>('telemetryTrends', { days });
          stopSpinner();
          if (result.count === 0) {
            console.log(`\n${DIM}No telemetry data for the last ${days} days.${RESET}\n`);
          } else {
            console.log(`\n${FG_ORANGE}${BOLD}Daily Trends${RESET} ${DIM}(last ${days} days)${RESET}\n`);
            console.log(`  ${DIM}Date         Turns  Tokens    Cost     Tools  Sessions${RESET}`);
            for (const d of result.daily) {
              const tokensK = (d.tokens / 1000).toFixed(1) + 'K';
              console.log(`  ${FG_WHITE}${d.date}${RESET}  ${String(d.turns).padStart(5)}  ${tokensK.padStart(7)}  $${d.cost.toFixed(3).padStart(7)}  ${String(d.tools).padStart(5)}  ${String(d.sessions).padStart(8)}`);
            }
            console.log();
          }
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      if (subCmd === 'export') {
        const format = parts[1] || 'summary';
        if (!['json', 'csv', 'summary', 'md', 'markdown'].includes(format)) {
          console.log(`\n${FG_YELLOW}Usage: /telemetry export <json|csv|summary>${RESET}\n`);
          rl.prompt();
          return;
        }
        startSpinner(`Exporting as ${format}...`);
        try {
          const result = await client.request<{ format: string; data: string }>('telemetryExport', { format });
          stopSpinner();
          console.log(`\n${FG_GREEN}✓ Exported (${format})${RESET}\n`);
          console.log(result.data);
          console.log();
        } catch (err) {
          stopSpinner();
          console.error(`${FG_RED}${err}${RESET}`);
        }
        rl.prompt();
        return;
      }

      // Falls through to /telemetry on|off handler
    }

    // ── /permission commands ──
    if (input.startsWith('/permission')) {
      const permArgs = input.slice('/permission'.length).trim();
      const parts = permArgs.split(/\s+/);
      const subCmd = parts[0] || '';

      if (subCmd === 'status' || subCmd === '') {
        try {
          const result = await client.request<{ rules: any[]; count: number }>('permissionStatus');
          console.log(`\n${FG_ORANGE}${BOLD}Permission Rules${RESET} ${DIM}(${result.count})${RESET}\n`);
          for (const r of result.rules) {
            const deny = r.auto_deny ? ` ${FG_RED}[deny]${RESET}` : '';
            const confirm = r.require_confirmation ? ` ${FG_YELLOW}[confirm]${RESET}` : '';
            const allow = (!r.auto_deny && !r.require_confirmation) ? ` ${FG_GREEN}[allow]${RESET}` : '';
            console.log(`  ${FG_WHITE}${r.tool}${RESET} ${DIM}${r.action}${RESET} ${DIM}→${RESET} ${r.target_pattern || '*'}${deny}${confirm}${allow}`);
            console.log(`    ${DIM}${r.description}${RESET}`);
          }
          console.log();
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (subCmd === 'grant') {
        const tool = parts[1];
        const action = parts[2];
        const target = parts[3];
        if (!tool || !action || !target) {
          console.log(`\n${FG_YELLOW}Usage: /permission grant <tool> <action> <target> [--permanent]${RESET}\n`);
          rl.prompt();
          return;
        }
        const permanent = parts.includes('--permanent');
        try {
          await client.request('permissionGrant', { tool, action, target, permanent });
          console.log(`\n${FG_GREEN}✓ Granted:${RESET} ${FG_WHITE}${tool} ${action} ${target}${RESET} ${DIM}${permanent ? '(permanent)' : '(session)'}${RESET}\n`);
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      if (subCmd === 'revoke') {
        const tool = parts[1];
        const action = parts[2];
        const target = parts[3];
        if (!tool || !action || !target) {
          console.log(`\n${FG_YELLOW}Usage: /permission revoke <tool> <action> <target>${RESET}\n`);
          rl.prompt();
          return;
        }
        try {
          const result = await client.request<{ success: boolean; removed: number }>('permissionRevoke', { tool, action, target });
          if (result.success) {
            console.log(`\n${FG_GREEN}✓ Revoked ${result.removed} grant(s)${RESET}\n`);
          } else {
            console.log(`\n${FG_YELLOW}No matching grants found${RESET}\n`);
          }
        } catch (err) { console.error(`${FG_RED}${err}${RESET}`); }
        rl.prompt();
        return;
      }

      console.log(`\n${FG_ORANGE}${BOLD}Permission Commands${RESET}\n`);
      console.log(`  ${FG_WHITE}/permission status${RESET}                        ${DIM}Show all permission rules${RESET}`);
      console.log(`  ${FG_WHITE}/permission grant <tool> <action> <target>${RESET}  ${DIM}Grant permission${RESET}`);
      console.log(`  ${FG_WHITE}/permission revoke <tool> <action> <target>${RESET} ${DIM}Revoke permission${RESET}\n`);
      rl.prompt();
      return;
    }

    if (input === '/help') {
      console.log(`\n${FG_ORANGE}${BOLD}Commands${RESET}\n`);

      console.log(`  ${FG_GRAY}── Conversation ──${RESET}`);
      console.log(`  ${FG_WHITE}/compact${RESET}    ${DIM}Compress conversation context${RESET}`);
      console.log(`  ${FG_WHITE}/think${RESET}      ${DIM}Toggle extended thinking mode${RESET}`);
      console.log(`  ${FG_WHITE}/model${RESET}      ${DIM}Show or switch model: /model list|route|budget${RESET}`);
      console.log(`  ${FG_WHITE}/history${RESET}    ${DIM}Recent conversation: /history [n]${RESET}`);
      console.log(`  ${FG_WHITE}/abort${RESET}      ${DIM}Cancel current request${RESET}`);
      console.log(`  ${FG_WHITE}/debug${RESET}      ${DIM}Toggle timing debug for next query${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Projects & Git ──${RESET}`);
      console.log(`  ${FG_WHITE}/projects${RESET}   ${DIM}List, switch, create projects${RESET}`);
      console.log(`  ${FG_WHITE}/git${RESET}        ${DIM}Git status (branch, changes)${RESET}`);
      console.log(`  ${FG_WHITE}/git pr list|create${RESET}   ${DIM}Pull request management${RESET}`);
      console.log(`  ${FG_WHITE}/git branch${RESET}  ${DIM}List branches${RESET}`);
      console.log(`  ${FG_WHITE}/git conflict${RESET}${DIM} Check for merge conflicts${RESET}`);
      console.log(`  ${FG_WHITE}/diff${RESET}       ${DIM}Git diff summary${RESET}`);
      console.log(`  ${FG_WHITE}/commit${RESET}     ${DIM}Stage all and commit${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Tools & Extensions ──${RESET}`);
      console.log(`  ${FG_WHITE}/tools${RESET}      ${DIM}List registered tools${RESET}`);
      console.log(`  ${FG_WHITE}/mcp${RESET}        ${DIM}List MCP servers${RESET}`);
      console.log(`  ${FG_WHITE}/skills${RESET}     ${DIM}List discovered skills${RESET}`);
      console.log(`  ${FG_WHITE}/plugins${RESET}    ${DIM}List discovered plugins${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Templates & Automation ──${RESET}`);
      console.log(`  ${FG_WHITE}/template${RESET}   ${DIM}Workflow templates: list, create, delete, export, import${RESET}`);
      console.log(`  ${FG_WHITE}/task${RESET}       ${DIM}Background tasks: run, list, status, stop${RESET}`);
      console.log(`  ${FG_WHITE}/cron${RESET}       ${DIM}Scheduled tasks: add, list, remove, toggle${RESET}`);
      console.log(`  ${FG_WHITE}/memory${RESET}     ${DIM}Long-term memory: list, add, delete, clear${RESET}`);
      console.log(`  ${FG_WHITE}/team${RESET}       ${DIM}Sub-agent teams: spawn, list, status, exec${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Telemetry & Permissions ──${RESET}`);
      console.log(`  ${FG_WHITE}/telemetry${RESET}  ${DIM}Telemetry: stats, trends, export, on|off${RESET}`);
      console.log(`  ${FG_WHITE}/permission${RESET} ${DIM}Permission gate: status, grant, revoke${RESET}`);
      console.log();

      console.log(`  ${FG_GRAY}── Input & Integrations ──${RESET}`);
      console.log(`  ${FG_WHITE}/doc${RESET}        ${DIM}Attach PDF/DOCX document for next message${RESET}`);
      console.log(`  ${FG_WHITE}/voice${RESET}      ${DIM}Voice input (requires whisper.cpp)${RESET}`);
      console.log(`  ${FG_WHITE}@file.pdf${RESET}   ${DIM}Attach file: @photo.png @doc.pdf @doc.docx${RESET}`);
      console.log(`  ${FG_WHITE}/telegram${RESET}   ${DIM}Manage Telegram gateway${RESET}`);
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

    // Auto-detect drag-drop image files (terminal pastes quoted path like '/path/to/img.png')
    const dragDropMatch = input.match(/^['"]?(\/[^\s'"]+\.(png|jpg|jpeg|gif|webp))['"]?\s*$/i);
    if (dragDropMatch) {
      const imgPath = dragDropMatch[1];
      if (fs.existsSync(imgPath)) {
        input = `@${imgPath}`;
      }
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
      // Merge pending attachments from /doc command
      if (pendingAttachments.length > 0) {
        const existing = (submitPayload.attachments as Array<Record<string, unknown>>) || [];
        submitPayload.attachments = [...existing, ...pendingAttachments];
        console.log(`${DIM}  📎 Including ${pendingAttachments.length} pending attachment(s) from /doc${RESET}`);
        pendingAttachments = [];
      }

      // Debug: record submit time for first query timing
      if (debugMode && !firstQueryDone) {
        resetDebugTimers();
        debugSubmitTime = Date.now();
      }
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
