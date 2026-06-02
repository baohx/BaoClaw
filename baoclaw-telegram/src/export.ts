/**
 * BaoClaw 对话导出模块 — 将 talkTail RPC 返回的对话条目格式化为 Markdown。
 * 逻辑与 baoclaw-core/src/engine/export.rs 和 baoclaw-web/src/export.ts 保持一致。
 */

export interface TranscriptEntry {
  role: 'user' | 'assistant';
  text: string;
  timestamp?: string;
  tools?: { name: string; detail?: string }[];
}

export interface ExportOptions {
  sessionId?: string;
  format?: 'markdown' | 'pdf';
  includeToolCalls?: boolean;
}

/**
 * Format a list of transcript entries into a Markdown document.
 *
 * Output follows the design spec format:
 * - Title and session metadata
 * - Each message as a section with timestamp
 * - Tool calls listed under assistant messages
 */
export function formatTranscriptToMarkdown(entries: TranscriptEntry[], options?: ExportOptions): string {
  const exportTime = new Date().toLocaleString('sv-SE').replace('T', ' ');
  const sessionId = options?.sessionId ?? '未知';
  const includeToolCalls = options?.includeToolCalls ?? true;

  let md = '';

  // Header
  md += '# BaoClaw 对话导出\n';
  md += `**会话**: ${sessionId}\n`;
  md += `**时间**: ${exportTime}\n`;
  md += `**消息数**: ${entries.length}\n`;
  md += '\n---\n\n';

  for (const entry of entries) {
    const ts = entry.timestamp ?? '';

    if (entry.role === 'user') {
      md += `## 用户 (${ts})\n`;
      md += entry.text;
      md += '\n';
    } else {
      md += `## 助手 (${ts})\n`;
      md += entry.text;
      md += '\n';

      // Render tool calls if present and enabled
      if (includeToolCalls && entry.tools && entry.tools.length > 0) {
        md += '\n### 工具调用\n';
        for (const tool of entry.tools) {
          const detail = tool.detail ? `: ${tool.detail}` : '';
          md += `- ⚡ ${tool.name}${detail}\n`;
        }
      }
    }

    md += '\n---\n\n';
  }

  return md;
}

/**
 * Generate a default export filename with current date.
 */
export function defaultExportFilename(): string {
  const now = new Date();
  const pad = (n: number) => n.toString().padStart(2, '0');
  const date = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `baoclaw-export-${date}.md`;
}

/**
 * Convert Markdown content to a PDF buffer.
 * TODO: Implement real PDF export using pdfkit or puppeteer.
 * Currently returns the markdown encoded as UTF-8 buffer as a placeholder.
 */
export function markdownToPdf(markdown: string): Buffer {
  // TODO: integrate pdfkit or similar for real PDF generation
  return Buffer.from(markdown, 'utf-8');
}
