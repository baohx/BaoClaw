/**
 * Message formatter for BaoClaw Feishu Gateway.
 *
 * Handles two input formats from the LLM:
 *   1. Pure Markdown (with code blocks, headings, lists, etc.)
 *   2. Mixed Markdown + HTML (tables, <details>, styled spans)
 *
 * Output: Cleaned Markdown suitable for Feishu's --markdown renderer.
 *
 * Feishu Markdown support:
 *   ✅ Bold: **text**     ✅ Italic: *text*        ✅ Code: `code`
 *   ✅ Code blocks: ```   ✅ Headings: # ## ###
 *   ✅ Lists: - / 1.      ✅ Links: [text](url)
 *   ✅ Blockquote: >      ✅ Strikethrough: ~~text~~
 *   ❌ Tables (use ASCII fallback)
 *   ❌ HTML tags (strip or convert)
 */

const MAX_MSG_LEN = 15000;

/**
 * Main entry: convert LLM output (Markdown + optional HTML) to Feishu-safe Markdown.
 */
export function formatForFeishu(text: string): string {
  let out = text;

  // 1. Handle HTML <details>/<summary> blocks → plain text with divider
  out = handleDetailsBlocks(out);

  // 2. Handle HTML tables → ASCII table
  out = handleHtmlTables(out);

  // 3. Strip dangerous/unsupported HTML tags but keep content
  out = stripHtmlTags(out);

  // 4. Convert remaining HTML entities
  out = decodeHtmlEntities(out);

  // 5. Clean excessive whitespace
  out = cleanWhitespace(out);

  return out.slice(0, MAX_MSG_LEN);
}

// ── HTML → Markdown conversions ──

/**
 * Convert <details><summary>X</summary>Y</details> to folded text blocks.
 */
function handleDetailsBlocks(text: string): string {
  // Match: <details>\s*<summary>...</summary>\s*...content...\s*</details>
  const re = /<details[^>]*>\s*\n?\s*<summary[^>]*>([\s\S]*?)<\/summary>\s*\n?\s*([\s\S]*?)<\/details>/gi;
  return text.replace(re, (_full, summary, content) => {
    const title = stripTags(summary).trim();
    const body = content.trim();
    if (!body) return `📋 **${title}**`;
    return `📋 **${title}**\n\n${body}\n\n---\n`;
  });
}

/**
 * Convert HTML <table> to ASCII table format.
 * Falls back to a simple row-by-row plain text if too wide.
 */
function handleHtmlTables(text: string): string {
  const tableRe = /<table[^>]*>([\s\S]*?)<\/table>/gi;
  return text.replace(tableRe, (_full, inner) => {
    const rows = parseTableRows(inner);
    if (rows.length === 0) return '';

    // Check if table fits in reasonable width
    const maxCols = Math.max(...rows.map(r => r.length));
    if (maxCols === 0) return '';

    // Calculate max width per column
    const colWidths = new Array(maxCols).fill(0);
    for (const row of rows) {
      for (let i = 0; i < row.length; i++) {
        colWidths[i] = Math.max(colWidths[i], row[i].length);
      }
    }

    // Cap column widths
    const totalWidth = colWidths.reduce((a, b) => a + b, 0) + maxCols * 3 + 1;
    if (totalWidth > 80) {
      // Too wide — use simple format
      let simple = '';
      for (const row of rows) {
        simple += row.join(' | ') + '\n';
      }
      return simple;
    }

    // ASCII table
    let table = '';
    const sep = '+' + colWidths.map(w => '-'.repeat(w + 2)).join('+') + '+\n';
    for (let i = 0; i < rows.length; i++) {
      if (i === 0) table += sep;
      table += '|';
      for (let j = 0; j < maxCols; j++) {
        const cell = rows[i][j] || '';
        table += ' ' + cell.padEnd(colWidths[j]) + ' |';
      }
      table += '\n';
      if (i === 0) table += sep;
    }
    table += sep;
    return table;
  });
}

/**
 * Parse <tr>/<td>/<th> from table inner HTML.
 */
function parseTableRows(html: string): string[][] {
  const rowRe = /<tr[^>]*>([\s\S]*?)<\/tr>/gi;
  const rows: string[][] = [];
  let rm: RegExpExecArray | null;
  while ((rm = rowRe.exec(html)) !== null) {
    const cellRe = /<t[dh][^>]*>([\s\S]*?)<\/t[dh]>/gi;
    const cells: string[] = [];
    let cm: RegExpExecArray | null;
    while ((cm = cellRe.exec(rm[1])) !== null) {
      cells.push(stripTags(cm[1]).replace(/\s+/g, ' ').trim());
    }
    if (cells.length > 0) rows.push(cells);
  }
  return rows;
}

// ── HTML stripping ──

/**
 * Strip unsafe HTML tags, converting known elements to markdown.
 * Preserves code blocks (```) untouched.
 */
function stripHtmlTags(text: string): string {
  // Protect code blocks first
  const blocks: string[] = [];
  const protected_ = text.replace(/```[\s\S]*?```/g, (m) => {
    blocks.push(m);
    return `\x00CODEBLOCK\x00${blocks.length - 1}\x00`;
  });

  let out = protected_;

  // Convert HTML bold to markdown
  out = out.replace(/<b>(.+?)<\/b>/gi, '**$1**');
  out = out.replace(/<strong>(.+?)<\/strong>/gi, '**$1**');

  // Convert HTML italic to markdown
  out = out.replace(/<i>(.+?)<\/i>/gi, '*$1*');
  out = out.replace(/<em>(.+?)<\/em>/gi, '*$1*');

  // Convert HTML code to markdown
  out = out.replace(/<code>(.+?)<\/code>/gi, '`$1`');

  // Convert <br>, <br/> to newlines
  out = out.replace(/<br\s*\/?>/gi, '\n');

  // Convert <li>...</li> to - ...
  out = out.replace(/<li>([\s\S]*?)<\/li>/gi, '- $1');

  // Convert <ol>/<ul> wrappers — just pass through content
  out = out.replace(/<\/?(?:ol|ul)[^>]*>/gi, '');

  // Convert <p> to newlines
  out = out.replace(/<p[^>]*>/gi, '\n');
  out = out.replace(/<\/p>/gi, '\n');

  // Convert blockquote
  out = out.replace(/<blockquote[^>]*>([\s\S]*?)<\/blockquote>/gi, (_m, content) => {
    return content.split('\n').map((l: string) => l.trim() ? '> ' + l.trim() : '>').join('\n');
  });

  // Convert <a href="url">text</a> to [text](url)
  out = out.replace(/<a\s+href="([^"]*)"[^>]*>([\s\S]*?)<\/a>/gi, '[$2]($1)');

  // Convert <img src="url" alt="text"> to ![text](url)
  out = out.replace(/<img\s+[^>]*src="([^"]*)"[^>]*alt="([^"]*)"[^>]*\/?>/gi, '![$2]($1)');
  out = out.replace(/<img\s+[^>]*alt="([^"]*)"[^>]*src="([^"]*)"[^>]*\/?>/gi, '![$1]($2)');

  // Convert <pre> blocks
  out = out.replace(/<pre[^>]*>([\s\S]*?)<\/pre>/gi, (_m, content) => {
    const stripped = stripTags(content).trim();
    return '```\n' + stripped + '\n```';
  });

  // Convert <div>, <span> — just pass through content
  out = out.replace(/<\/?div[^>]*>/gi, '\n');
  out = out.replace(/<\/?span[^>]*>/gi, '');

  // Strip remaining HTML tags (keep content)
  out = out.replace(/<\/?[a-zA-Z][a-zA-Z0-9]*[^>]*>/g, '');

  // Restore code blocks
  out = out.replace(/\x00CODEBLOCK\x00(\d+)\x00/g, (_m, idx) => {
    return blocks[parseInt(idx, 10)] || '';
  });

  return out;
}

// ── HTML entity decoding ──

function decodeHtmlEntities(text: string): string {
  return text
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&nbsp;/g, ' ')
    .replace(/&#x27;/g, "'");
}

// ── Whitespace cleanup ──

function cleanWhitespace(text: string): string {
  // Collapse 3+ consecutive newlines to 2
  let out = text.replace(/\n{3,}/g, '\n\n');
  // Remove trailing whitespace per line
  out = out.replace(/[ \t]+$/gm, '');
  return out.trim();
}

// ── Utility ──

function stripTags(html: string): string {
  return html.replace(/<[^>]*>/g, '');
}

// ── Split for Feishu length limit ──

export function splitMessage(text: string, maxLength: number = 15000): string[] {
  if (text.length <= maxLength) return [text];

  const chunks: string[] = [];
  let remaining = text;

  while (remaining.length > maxLength) {
    // Try code block boundary
    const region = remaining.slice(0, maxLength);
    let splitIdx = region.lastIndexOf('\n```\n');
    if (splitIdx > 0) { splitIdx += 5; }

    // Try paragraph boundary
    if (splitIdx <= 0) {
      const paraIdx = region.lastIndexOf('\n\n');
      if (paraIdx > 0) splitIdx = paraIdx + 2;
    }

    // Try line boundary
    if (splitIdx <= 0) {
      const lineIdx = region.lastIndexOf('\n');
      if (lineIdx > 0) splitIdx = lineIdx + 1;
    }

    // Hard split
    if (splitIdx <= 0) splitIdx = maxLength;

    chunks.push(remaining.slice(0, splitIdx));
    remaining = remaining.slice(splitIdx);
  }

  if (remaining.length > 0) chunks.push(remaining);
  return chunks;
}
