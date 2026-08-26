/**
 * Message formatter for WhatsApp Gateway.
 * Converts BaoClaw Markdown output to WhatsApp-compatible formatting.
 *
 * Conversions:
 *   Markdown bold **text** → WhatsApp bold *text*
 *   Markdown italic *text* or _text_ → WhatsApp italic _text_
 *   Code blocks (triple backtick) → preserved as-is (WhatsApp supports them)
 *   Markdown headings ## / ### → WhatsApp bold *text*
 *   Markdown links [text](url) → url
 *   Checkbox tasks - [x] / - [ ] → ✅ / ☐
 */

/**
 * Convert Markdown formatting to WhatsApp formatting.
 * - **bold** → *bold*
 * - *italic* (single asterisk) → _italic_
 * - _italic_ → _italic_ (already WhatsApp format)
 * - ```code``` → ```code``` (preserved)
 * - ## heading → *heading*
 * - [text](url) → url
 * - - [x] text → ✅ text
 * - - [ ] text → ☐ text
 */
export function formatForWhatsApp(markdown: string): string {
  let result = "";
  let i = 0;
  const len = markdown.length;

  while (i < len) {
    // Code block: preserve as-is
    if (markdown.startsWith("```", i)) {
      const endIdx = markdown.indexOf("```", i + 3);
      if (endIdx >= 0) {
        result += markdown.slice(i, endIdx + 3);
        i = endIdx + 3;
        continue;
      }
      // No closing — just output the rest
      result += markdown.slice(i);
      break;
    }

    // Markdown heading → WhatsApp bold: ## text → *text*
    // Must be at start of a line
    if (
      (i === 0 || markdown[i - 1] === "\n") &&
      markdown.startsWith("## ", i)
    ) {
      // Find end of line
      const lineEnd = markdown.indexOf("\n", i + 3);
      const headingText =
        lineEnd >= 0 ? markdown.slice(i + 3, lineEnd) : markdown.slice(i + 3);
      const trimmed = headingText.trim();
      result += "*" + trimmed + "*";
      i = lineEnd >= 0 ? lineEnd + 1 : len;
      continue;
    }
    // Also handle ### heading
    if (
      (i === 0 || markdown[i - 1] === "\n") &&
      markdown.startsWith("### ", i)
    ) {
      const lineEnd = markdown.indexOf("\n", i + 4);
      const headingText =
        lineEnd >= 0 ? markdown.slice(i + 4, lineEnd) : markdown.slice(i + 4);
      const trimmed = headingText.trim();
      result += "*" + trimmed + "*";
      i = lineEnd >= 0 ? lineEnd + 1 : len;
      continue;
    }

    // Checkbox task → emoji: - [x] text → ✅ text, - [ ] text → ☐ text
    if (
      (i === 0 || markdown[i - 1] === "\n") &&
      markdown.startsWith("- [", i)
    ) {
      if (i + 5 < len && markdown[i + 3] === "x" && markdown[i + 4] === "]") {
        result += "✅ " + markdown.slice(i + 5).replace(/^\s+/, "");
        i += 5;
        // Skip to end of line
        const lineEnd = markdown.indexOf("\n", i);
        if (lineEnd >= 0) {
          i = lineEnd; // don't skip the \n, let the main loop handle it
        } else {
          i = len;
        }
        continue;
      }
      if (i + 5 < len && markdown[i + 3] === " " && markdown[i + 4] === "]") {
        result += "☐ " + markdown.slice(i + 5).replace(/^\s+/, "");
        i += 5;
        const lineEnd = markdown.indexOf("\n", i);
        if (lineEnd >= 0) {
          i = lineEnd;
        } else {
          i = len;
        }
        continue;
      }
    }

    // Inline code: preserve as-is
    if (markdown[i] === "`") {
      const endIdx = markdown.indexOf("`", i + 1);
      if (endIdx >= 0) {
        result += markdown.slice(i, endIdx + 1);
        i = endIdx + 1;
        continue;
      }
    }

    // Markdown link: [text](url) → url
    if (markdown[i] === "[") {
      const linkMatch = matchLink(markdown, i);
      if (linkMatch) {
        result += linkMatch.url;
        i = linkMatch.endIdx;
        continue;
      }
    }

    // Bold: **text** → *text*
    if (markdown.startsWith("**", i)) {
      const endIdx = markdown.indexOf("**", i + 2);
      if (endIdx >= 0) {
        const inner = markdown.slice(i + 2, endIdx);
        result += "*" + inner + "*";
        i = endIdx + 2;
        continue;
      }
    }

    // Italic with single asterisk: *text* → _text_
    if (markdown[i] === "*" && !markdown.startsWith("**", i)) {
      const endIdx = findClosingMarker(markdown, i + 1, "*");
      if (endIdx >= 0) {
        const inner = markdown.slice(i + 1, endIdx);
        result += "_" + inner + "_";
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

/** Try to match a Markdown link [text](url) at position `start`. */
function matchLink(
  text: string,
  start: number,
): { url: string; endIdx: number } | null {
  // Must start with [
  if (text[start] !== "[") return null;
  // Find closing ]
  let depth = 0;
  let j = start;
  while (j < text.length) {
    if (text[j] === "[") depth++;
    if (text[j] === "]") {
      depth--;
      if (depth === 0) break;
    }
    j++;
  }
  if (depth !== 0) return null;
  const closeBracket = j;
  // Next char must be (
  if (closeBracket + 1 >= text.length || text[closeBracket + 1] !== "(")
    return null;
  // Find closing )
  const parenEnd = text.indexOf(")", closeBracket + 2);
  if (parenEnd < 0) return null;
  const url = text.slice(closeBracket + 2, parenEnd);
  return { url, endIdx: parenEnd + 1 };
}

/** Find the next occurrence of a single marker that isn't doubled. */
function findClosingMarker(
  text: string,
  start: number,
  marker: string,
): number {
  for (let i = start; i < text.length; i++) {
    if (text[i] === marker && (marker !== "*" || !text.startsWith("**", i))) {
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
export function splitMessage(text: string, maxLength: number = 4000): string[] {
  if (text.length <= maxLength) return [text];

  const chunks: string[] = [];
  let remaining = text;

  while (remaining.length > maxLength) {
    let splitIdx = -1;

    // Try paragraph boundary
    const searchRegion = remaining.slice(0, maxLength);
    const paraIdx = searchRegion.lastIndexOf("\n\n");
    if (paraIdx > 0) {
      splitIdx = paraIdx + 2; // include the double newline in the first chunk
    }

    // Try line boundary
    if (splitIdx < 0) {
      const lineIdx = searchRegion.lastIndexOf("\n");
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
