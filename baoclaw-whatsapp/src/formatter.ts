/**
 * Message formatter for WhatsApp Gateway.
 * Converts BaoClaw Markdown output to WhatsApp-compatible formatting.
 *
 * Conversions:
 *   Markdown bold **text** → WhatsApp bold *text*
 *   Markdown italic *text* or _text_ → WhatsApp italic _text_
 *   Code blocks (triple backtick) → preserved as-is (WhatsApp supports them)
 */

/**
 * Convert Markdown formatting to WhatsApp formatting.
 * - **bold** → *bold*
 * - *italic* (single asterisk) → _italic_
 * - _italic_ → _italic_ (already WhatsApp format)
 * - ```code``` → ```code``` (preserved)
 */
export function formatForWhatsApp(markdown: string): string {
  let result = '';
  let i = 0;
  const len = markdown.length;

  while (i < len) {
    // Code block: preserve as-is
    if (markdown.startsWith('```', i)) {
      const endIdx = markdown.indexOf('```', i + 3);
      if (endIdx >= 0) {
        result += markdown.slice(i, endIdx + 3);
        i = endIdx + 3;
        continue;
      }
      // No closing — just output the rest
      result += markdown.slice(i);
      break;
    }

    // Inline code: preserve as-is
    if (markdown[i] === '`') {
      const endIdx = markdown.indexOf('`', i + 1);
      if (endIdx >= 0) {
        result += markdown.slice(i, endIdx + 1);
        i = endIdx + 1;
        continue;
      }
    }

    // Bold: **text** → *text*
    if (markdown.startsWith('**', i)) {
      const endIdx = markdown.indexOf('**', i + 2);
      if (endIdx >= 0) {
        const inner = markdown.slice(i + 2, endIdx);
        result += '*' + inner + '*';
        i = endIdx + 2;
        continue;
      }
    }

    // Italic with single asterisk: *text* → _text_
    if (markdown[i] === '*' && !markdown.startsWith('**', i)) {
      const endIdx = findClosingMarker(markdown, i + 1, '*');
      if (endIdx >= 0) {
        const inner = markdown.slice(i + 1, endIdx);
        result += '_' + inner + '_';
        i = endIdx + 1;
        continue;
      }
    }

    // _italic_ → _italic_ (already WhatsApp format, pass through)
    result += markdown[i];
    i++;
  }

  return result;
}

/** Find the next occurrence of a single marker that isn't doubled. */
function findClosingMarker(text: string, start: number, marker: string): number {
  for (let i = start; i < text.length; i++) {
    if (text[i] === marker && (marker !== '*' || !text.startsWith('**', i))) {
      return i;
    }
  }
  return -1;
}

/**
 * Split a message into chunks of at most maxLength characters.
 * Tries to split at paragraph boundaries (\n\n), then line boundaries (\n),
 * then at maxLength as a last resort.
 * Concatenating all chunks reproduces the original text.
 */
export function splitMessage(text: string, maxLength: number = 4096): string[] {
  if (text.length <= maxLength) return [text];

  const chunks: string[] = [];
  let remaining = text;

  while (remaining.length > maxLength) {
    let splitIdx = -1;

    // Try paragraph boundary
    const searchRegion = remaining.slice(0, maxLength);
    const paraIdx = searchRegion.lastIndexOf('\n\n');
    if (paraIdx > 0) {
      splitIdx = paraIdx + 2; // include the double newline in the first chunk
    }

    // Try line boundary
    if (splitIdx < 0) {
      const lineIdx = searchRegion.lastIndexOf('\n');
      if (lineIdx > 0) {
        splitIdx = lineIdx + 1;
      }
    }

    // Hard split
    if (splitIdx < 0) {
      splitIdx = maxLength;
    }

    chunks.push(remaining.slice(0, splitIdx));
    remaining = remaining.slice(splitIdx);
  }

  if (remaining.length > 0) {
    chunks.push(remaining);
  }

  return chunks;
}

/**
 * Format a tool use notification for WhatsApp.
 */
export function formatToolUse(toolName: string): string {
  return `⚡ ${toolName}`;
}

/**
 * Format an error message for WhatsApp.
 */
export function formatError(code: string, message: string): string {
  return `❌ [${code}] ${message}`;
}
