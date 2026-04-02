/**
 * Terminal Markdown renderer using ANSI escape codes.
 * Renders Markdown text with syntax highlighting, tables, lists, etc.
 */

const ESC = '\x1b[';
const RESET = `${ESC}0m`;
const BOLD = `${ESC}1m`;
const DIM = `${ESC}2m`;
const ITALIC = `${ESC}3m`;
const UNDERLINE = `${ESC}4m`;

const FG_ORANGE = `${ESC}38;2;217;119;40m`;
const FG_BLUE = `${ESC}34m`;
const FG_GREEN = `${ESC}32m`;
const FG_GRAY = `${ESC}90m`;
const FG_WHITE = `${ESC}37m`;
const FG_CYAN = `${ESC}36m`;
const FG_YELLOW = `${ESC}33m`;
const BG_CODE = `${ESC}48;2;40;40;40m`;

// Common keywords for syntax highlighting
const KEYWORDS = new Set([
  'function', 'const', 'let', 'var', 'if', 'else', 'for', 'while', 'return',
  'import', 'export', 'from', 'class', 'new', 'this', 'async', 'await',
  'try', 'catch', 'throw', 'switch', 'case', 'break', 'continue', 'default',
  'typeof', 'instanceof', 'in', 'of', 'true', 'false', 'null', 'undefined',
  'fn', 'pub', 'use', 'mod', 'struct', 'enum', 'impl', 'trait', 'match',
  'self', 'super', 'crate', 'mut', 'ref', 'type', 'where', 'async', 'move',
  'def', 'print', 'with', 'as', 'is', 'not', 'and', 'or', 'None', 'True', 'False',
]);

/**
 * Apply basic syntax highlighting to a code line.
 */
function highlightCodeLine(line: string): string {
  let result = '';
  let i = 0;
  while (i < line.length) {
    // String literals (double or single quotes)
    if (line[i] === '"' || line[i] === "'") {
      const quote = line[i];
      let end = i + 1;
      while (end < line.length && line[end] !== quote) {
        if (line[end] === '\\') end++; // skip escaped char
        end++;
      }
      if (end < line.length) end++; // include closing quote
      result += `${FG_GREEN}${line.slice(i, end)}${RESET}${BG_CODE}`;
      i = end;
      continue;
    }
    // Line comments
    if (line[i] === '/' && i + 1 < line.length && line[i + 1] === '/') {
      result += `${FG_GRAY}${line.slice(i)}${RESET}${BG_CODE}`;
      break;
    }
    if (line[i] === '#' && (i === 0 || line[i - 1] === ' ')) {
      // Python-style comment (only if at start or after space)
      result += `${FG_GRAY}${line.slice(i)}${RESET}${BG_CODE}`;
      break;
    }
    // Keywords
    if (/[a-zA-Z_]/.test(line[i])) {
      let end = i;
      while (end < line.length && /[a-zA-Z0-9_]/.test(line[end])) end++;
      const word = line.slice(i, end);
      if (KEYWORDS.has(word)) {
        result += `${FG_BLUE}${word}${RESET}${BG_CODE}`;
      } else {
        result += word;
      }
      i = end;
      continue;
    }
    result += line[i];
    i++;
  }
  return result;
}


/**
 * Render a code block with syntax highlighting.
 */
function renderCodeBlock(lines: string[], lang: string): string {
  const cols = process.stdout.columns || 80;
  const width = Math.min(cols - 4, 100);
  const topBar = lang
    ? `${DIM}${FG_GRAY}╭─ ${lang} ${'─'.repeat(Math.max(0, width - lang.length - 4))}╮${RESET}`
    : `${DIM}${FG_GRAY}╭${'─'.repeat(width)}╮${RESET}`;
  const botBar = `${DIM}${FG_GRAY}╰${'─'.repeat(width)}╯${RESET}`;

  const rendered = lines.map(l => {
    const highlighted = highlightCodeLine(l);
    return `${DIM}${FG_GRAY}│${RESET} ${BG_CODE}${highlighted}${RESET}`;
  });

  return [topBar, ...rendered, botBar].join('\n');
}

/**
 * Render a Markdown table with box-drawing characters.
 */
function renderTable(rows: string[][]): string {
  if (rows.length === 0) return '';

  // Calculate column widths
  const colCount = Math.max(...rows.map(r => r.length));
  const colWidths: number[] = Array(colCount).fill(0);
  for (const row of rows) {
    for (let c = 0; c < row.length; c++) {
      colWidths[c] = Math.max(colWidths[c], (row[c] || '').trim().length);
    }
  }

  const hLine = (left: string, mid: string, right: string) => {
    return `${FG_GRAY}${left}${colWidths.map(w => '─'.repeat(w + 2)).join(mid)}${right}${RESET}`;
  };

  const formatRow = (row: string[]) => {
    const cells = colWidths.map((w, i) => {
      const cell = (row[i] || '').trim();
      return ` ${cell}${' '.repeat(Math.max(0, w - cell.length))} `;
    });
    return `${FG_GRAY}│${RESET}${cells.join(`${FG_GRAY}│${RESET}`)}${FG_GRAY}│${RESET}`;
  };

  const output: string[] = [];
  output.push(hLine('┌', '┬', '┐'));

  for (let i = 0; i < rows.length; i++) {
    // Skip separator rows (e.g., |---|---|)
    const isSeparator = rows[i].every(cell => /^[\s\-:]+$/.test(cell || ''));
    if (isSeparator) {
      output.push(hLine('├', '┼', '┤'));
      continue;
    }
    output.push(formatRow(rows[i]));
  }

  output.push(hLine('└', '┴', '┘'));
  return output.join('\n');
}

/**
 * Render Markdown text to ANSI-formatted terminal output.
 */
export function renderMarkdown(text: string): string {
  const lines = text.split('\n');
  const output: string[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    // Code blocks
    if (line.trimStart().startsWith('```')) {
      const lang = line.trimStart().slice(3).trim();
      const codeLines: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trimStart().startsWith('```')) {
        codeLines.push(lines[i]);
        i++;
      }
      i++; // skip closing ```
      output.push(renderCodeBlock(codeLines, lang));
      continue;
    }

    // Tables: detect lines starting with |
    if (line.trimStart().startsWith('|')) {
      const tableRows: string[][] = [];
      while (i < lines.length && lines[i].trimStart().startsWith('|')) {
        const cells = lines[i].split('|').slice(1); // remove leading empty
        if (cells.length > 0 && cells[cells.length - 1].trim() === '') {
          cells.pop(); // remove trailing empty
        }
        tableRows.push(cells);
        i++;
      }
      output.push(renderTable(tableRows));
      continue;
    }

    // Horizontal rule
    if (/^(\s*)(---+|===+|\*\*\*+)\s*$/.test(line)) {
      const cols = process.stdout.columns || 80;
      output.push(`${FG_GRAY}${'─'.repeat(Math.min(cols - 2, 70))}${RESET}`);
      i++;
      continue;
    }

    // Headings
    if (line.startsWith('### ')) {
      output.push(`${BOLD}${DIM}${line.slice(4)}${RESET}`);
      i++;
      continue;
    }
    if (line.startsWith('## ')) {
      output.push(`${BOLD}${FG_WHITE}${line.slice(3)}${RESET}`);
      i++;
      continue;
    }
    if (line.startsWith('# ')) {
      output.push(`${BOLD}${FG_ORANGE}${line.slice(2)}${RESET}`);
      i++;
      continue;
    }

    // Unordered lists
    if (/^\s*[-*]\s/.test(line)) {
      const match = line.match(/^(\s*)[-*]\s(.*)$/);
      if (match) {
        const indent = match[1] || '';
        const content = renderInline(match[2]);
        output.push(`${indent}  • ${content}`);
        i++;
        continue;
      }
    }

    // Ordered lists
    if (/^\s*\d+\.\s/.test(line)) {
      const match = line.match(/^(\s*)(\d+)\.\s(.*)$/);
      if (match) {
        const indent = match[1] || '';
        const num = match[2];
        const content = renderInline(match[3]);
        output.push(`${indent}  ${num}. ${content}`);
        i++;
        continue;
      }
    }

    // Regular line with inline formatting
    output.push(renderInline(line));
    i++;
  }

  return output.join('\n');
}

/**
 * Render inline Markdown formatting (bold, inline code, links).
 */
function renderInline(text: string): string {
  // Bold: **text**
  text = text.replace(/\*\*(.+?)\*\*/g, `${BOLD}$1${RESET}`);
  // Inline code: `code`
  text = text.replace(/`([^`]+)`/g, `${DIM}${BG_CODE} $1 ${RESET}`);
  // Links: [text](url)
  text = text.replace(/\[([^\]]+)\]\(([^)]+)\)/g, `${UNDERLINE}${FG_BLUE}$1${RESET}`);
  return text;
}
