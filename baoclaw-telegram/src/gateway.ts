/**
 * BaoClaw Telegram Gateway — connects to the daemon as a second client via UDS.
 * Each connection gets its own QueryEngine with independent conversation history.
 * The gateway is a SEPARATE process from the daemon and CLI.
 */
import * as fs from "fs";
import * as path from "path";
import * as os from "os";
import {
  DaemonConnector,
  IpcClient,
  resolveFixedSocket,
  selectNewestDaemon,
  type DaemonInfo,
} from "../../ts-ipc/index.js";
import { createLogger } from "../../ts-ipc/logger.js";
import { securePrivateFile } from "../../ts-ipc/security.js";
import {
  Bot,
  InputFile,
  type Context,
  type Message,
  type User,
} from "node-telegram-bot-api";
import { fromPath, run } from "node-telegram-bot-api/node";
import {
  parseDocument,
  buildDocumentBlock,
  buildImageBlock,
} from "./docParser.js";
import { formatTranscriptToMarkdown, defaultExportFilename } from "./export.js";
import {
  SessionState,
  InitializeResult,
  SearchResult,
  parseCommand,
  isRegisteredCommand,
  COMMAND_REGISTRY,
  formatTools,
  formatSkills,
  formatMcpServers,
  formatPlugins,
  formatCompact,
  formatGitStatus,
  formatGitDiff,
  formatGitCommit,
  formatThinkToggle,
  formatModelInfo,
  formatModelSwitch,
  formatCommitUsage,
  formatAbortConfirm,
  formatError,
  formatDisconnected,
  formatHelp,
  formatStatus,
  formatStart,
  formatSearchResults,
} from "./commands.js";
import { splitMessage } from "./messageSplitter.js";
import { isAllowedChat } from "./authorization.js";

const logger = createLogger("telegram");

// ── Global error handlers ──
process.on("uncaughtException", (err) => {
  logger.error(`UNCAUGHT: ${String(err)}`);
  process.exit(1);
});
process.on("unhandledRejection", (err) => {
  logger.error(`UNHANDLED REJECTION: ${String(err)}`);
  process.exit(1);
});

const PID_FILE = path.join(os.homedir(), ".baoclaw", "telegram-gateway.pid");
const CONFIG_PATH = path.join(os.homedir(), ".baoclaw", "config.json");
const MAX_TG_MSG = 4096;

// ═══════════════════════════════════════════════════════════════
// Config
// ═══════════════════════════════════════════════════════════════
interface TelegramConfig {
  token: string;
  allowedChatIds: number[];
}

function loadConfig(): TelegramConfig {
  let raw: any = {};
  securePrivateFile(CONFIG_PATH);
  try {
    raw = JSON.parse(fs.readFileSync(CONFIG_PATH, "utf-8"));
  } catch {}
  const tg = raw?.telegram ?? {};
  return {
    token: tg.token || process.env.TELEGRAM_BOT_TOKEN || "",
    allowedChatIds: Array.isArray(tg.allowedChatIds)
      ? tg.allowedChatIds.filter(
          (id: unknown): id is number =>
            typeof id === "number" && Number.isSafeInteger(id),
        )
      : [],
  };
}

// ═══════════════════════════════════════════════════════════════
// Daemon discovery — shared implementation lives in ts-ipc
// (IpcClient, DaemonInfo, discovery helpers imported above)
// ═══════════════════════════════════════════════════════════════

/**
 * Connect to daemon with retry. Waits up to maxWaitMs for a daemon to appear.
 * Kept local because Telegram overrides the initialize cwd via
 * BAOCLAW_TELEGRAM_CWD and derives a richer SessionState from the response.
 */
async function connectToDaemon(
  maxWaitMs = 60_000,
  retryIntervalMs = 5_000,
): Promise<{
  client: IpcClient;
  info: DaemonInfo;
  sessionState: SessionState;
  connector: DaemonConnector;
}> {
  const connector = new DaemonConnector({ sessionTag: "telegram" });
  const deadline = Date.now() + maxWaitMs;
  let lastError: Error | null = null;
  while (Date.now() < deadline) {
    const fixedSocket = resolveFixedSocket();
    if (fixedSocket && fs.existsSync(fixedSocket)) {
      const fixedInfo: DaemonInfo = {
        pid: 0,
        cwd: process.cwd(),
        session_id: "telegram",
        socket: fixedSocket,
        started_at: new Date().toISOString(),
      };
      try {
        const client = new IpcClient({ requestTimeoutMs: 0 });
        await client.connect(fixedSocket);
        const telegramCwd = process.env.BAOCLAW_TELEGRAM_CWD || process.cwd();
        const result = await client.request<InitializeResult>("initialize", {
          cwd: telegramCwd,
          settings: {},
          shared_session_id: "telegram",
        });
        const sessionState: SessionState = {
          resumed: Boolean(result?.resumed),
          messageCount: result?.message_count ?? 0,
          sessionId: result?.session_id ?? "telegram",
          shared: Boolean(result?.shared),
        };
        return { client, info: fixedInfo, sessionState, connector };
      } catch (err) {
        lastError = err instanceof Error ? err : new Error(String(err));
        logger.info(
          `Fixed socket connection attempt failed: ${lastError.message}`,
        );
      }
    }
    const best = selectNewestDaemon(connector.discover());
    if (best) {
      try {
        const client = new IpcClient({ requestTimeoutMs: 0 });
        await client.connect(best.socket);
        // Use CLI's cwd if available (from /telegram start), else daemon's cwd
        const telegramCwd = process.env.BAOCLAW_TELEGRAM_CWD || best.cwd;
        const result = await client.request<InitializeResult>("initialize", {
          cwd: telegramCwd,
          settings: {},
          shared_session_id: "telegram",
        });
        let sessionState: SessionState = {
          resumed: false,
          messageCount: 0,
          sessionId: result?.session_id ?? best.session_id,
          shared: result?.shared ?? false,
        };
        try {
          if (result && result.resumed) {
            sessionState = {
              resumed: true,
              messageCount: result.message_count ?? 0,
              sessionId: result.session_id ?? best.session_id,
              shared: result?.shared ?? false,
            };
            logger.info(
              `Resumed session ${sessionState.sessionId} (${sessionState.messageCount} messages)`,
            );
          }
          if (sessionState.shared) {
            logger.info(
              `Joined shared session ${sessionState.sessionId} (${sessionState.messageCount} messages)`,
            );
          }
        } catch {
          // Resume extraction failed — silently degrade to new session
        }
        return { client, info: best, sessionState, connector };
      } catch (err) {
        lastError = err instanceof Error ? err : new Error(String(err));
        logger.info(`Connection attempt failed: ${err}. Retrying...`);
      }
    } else {
      logger.info("No daemon found. Waiting...");
    }
    await new Promise((r) => setTimeout(r, retryIntervalMs));
  }
  const detail = lastError ? ` Last error: ${lastError.message}` : "";
  throw new Error(
    `No BaoClaw daemon found after ${maxWaitMs / 1000}s. Start one with: baoclaw.${detail}`,
  );
}

// ═══════════════════════════════════════════════════════════════
// Per-chat message queue (one message at a time per chat)
// ═══════════════════════════════════════════════════════════════
class ChatQueue {
  private queues = new Map<number, string[]>();
  private processing = new Set<number>();

  enqueue(chatId: number, text: string): void {
    const q = this.queues.get(chatId) ?? [];
    q.push(text);
    this.queues.set(chatId, q);
  }

  dequeue(chatId: number): string | undefined {
    const q = this.queues.get(chatId);
    if (!q || q.length === 0) return undefined;
    return q.shift();
  }

  hasQueued(chatId: number): boolean {
    const q = this.queues.get(chatId);
    return !!q && q.length > 0;
  }

  isProcessing(chatId: number): boolean {
    return this.processing.has(chatId);
  }

  startProcessing(chatId: number): void {
    this.processing.add(chatId);
  }

  finishProcessing(chatId: number): void {
    this.processing.delete(chatId);
  }
}

// ═══════════════════════════════════════════════════════════════
// Markdown → Telegram HTML converter
// ═══════════════════════════════════════════════════════════════

/**
 * Convert markdown-like text to Telegram-safe HTML.
 * Escapes raw HTML first, then applies safe formatting tags.
 */
function markdownToTelegramHtml(text: string): string {
  // 1. Escape HTML entities first (so raw model HTML doesn't break Telegram)
  let html = text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  // 2. Code blocks: ```lang\n...\n``` → <pre><code class="language-lang">...</code></pre>
  html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_m, lang, code) => {
    const cls = lang ? ` class="language-${lang}"` : "";
    return `<pre><code${cls}>${code.trimEnd()}</code></pre>`;
  });

  // 3. Inline code: `code` → <code>code</code>
  html = html.replace(/`([^`\n]+)`/g, "<code>$1</code>");

  // 4. Bold: **text** → <b>text</b>
  html = html.replace(/\*\*(.+?)\*\*/g, "<b>$1</b>");

  // 5. Italic: *text* → <i>text</i> (but not inside bold)
  html = html.replace(/(?<!\*)\*(?!\*)(.+?)(?<!\*)\*(?!\*)/g, "<i>$1</i>");

  // 6. Strikethrough: ~~text~~ → <s>text</s>
  html = html.replace(/~~(.+?)~~/g, "<s>$1</s>");

  // 7. Links: [text](url) → <a href="url">text</a>
  html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>');

  return html;
}

// ═══════════════════════════════════════════════════════════════
// Base64 image extraction
// ═══════════════════════════════════════════════════════════════
interface ExtractedImage {
  buffer: Buffer;
  caption?: string;
}

function extractBase64Images(text: string): {
  text: string;
  images: ExtractedImage[];
} {
  const images: ExtractedImage[] = [];
  let cleaned = text;

  // 1. Markdown image syntax: ![alt](data:image/...;base64,...)
  const mdImgRegex =
    /!\[([^\]]*)\]\(data:image\/(png|jpeg|jpg|gif|webp);base64,([A-Za-z0-9+/=\s]+)\)/g;
  let match: RegExpExecArray | null;
  while ((match = mdImgRegex.exec(text)) !== null) {
    try {
      const base64Data = match[3].replace(/\s/g, "");
      const buffer = Buffer.from(base64Data, "base64");
      if (buffer.length > 100) {
        images.push({ buffer, caption: match[1] || undefined });
      }
    } catch {
      /* skip */
    }
  }
  cleaned = cleaned.replace(mdImgRegex, "");

  // 2. MCP content format: {"type":"image","data":"base64...","mimeType":"image/png"}
  // Also handles arrays: [{"type":"image",...}]
  try {
    const parsed = JSON.parse(cleaned);
    const contents = Array.isArray(parsed?.content)
      ? parsed.content
      : Array.isArray(parsed)
        ? parsed
        : [];
    for (const item of contents) {
      if (item?.type === "image" && item?.data) {
        try {
          const buffer = Buffer.from(item.data, "base64");
          if (buffer.length > 100) {
            images.push({ buffer, caption: "📸 Screenshot" });
          }
        } catch {
          /* skip */
        }
      }
    }
    if (images.length > 0 && contents.length > 0) {
      // Extract text content from MCP response
      const textParts = contents
        .filter((c: any) => c?.type === "text")
        .map((c: any) => c.text || "");
      cleaned = textParts.join("\n");
    }
  } catch {
    /* not JSON, continue */
  }

  // 3. Standalone data URIs not in markdown syntax
  const dataUriRegex =
    /data:image\/(png|jpeg|jpg|gif|webp);base64,([A-Za-z0-9+/=\s]+)/g;
  while ((match = dataUriRegex.exec(cleaned)) !== null) {
    try {
      const base64Data = match[2].replace(/\s/g, "");
      const buffer = Buffer.from(base64Data, "base64");
      if (buffer.length > 100) {
        images.push({ buffer });
      }
    } catch {
      /* skip */
    }
  }
  cleaned = cleaned.replace(dataUriRegex, "[image]");

  // 4. Clean up very long base64 blocks that might have been missed
  cleaned = cleaned.replace(/[A-Za-z0-9+/=]{500,}/g, "[image data]");

  // 5. Clean up empty markdown image remnants
  cleaned = cleaned
    .replace(/!\[\]\(\)/g, "")
    .replace(/!\[[^\]]*\]\(\s*\)/g, "");

  return { text: cleaned.trim(), images };
}

// ═══════════════════════════════════════════════════════════════
// Main gateway
// ═══════════════════════════════════════════════════════════════
async function main() {
  const config = loadConfig();

  if (!config.token) {
    logger.error("Error: Telegram bot token not set.");
    logger.error(
      "Set telegram.token in ~/.baoclaw/config.json or TELEGRAM_BOT_TOKEN env var.",
    );
    process.exit(1);
  }
  if (config.allowedChatIds.length === 0) {
    logger.error(
      "Cannot start because no chat allowlist is configured. To fix, set allowedChatIds in config.json.",
    );
    process.exit(1);
  }

  logger.info("BaoClaw Telegram Gateway starting (daemon mode)...");

  // ── Discover and connect to daemon ──
  logger.info("Discovering BaoClaw daemon...");
  let ipcClient: IpcClient;
  let daemonInfo: DaemonInfo;
  let sessionState: SessionState;
  let daemonConnector: DaemonConnector;
  try {
    const conn = await connectToDaemon();
    ipcClient = conn.client;
    daemonInfo = conn.info;
    daemonConnector = conn.connector;
    sessionState = conn.sessionState;
    logger.info(
      `Connected to daemon pid=${daemonInfo.pid} cwd=${daemonInfo.cwd} session=${daemonInfo.session_id}`,
    );
  } catch (err: any) {
    logger.error(`Failed to connect to daemon: ${err.message}`);
    process.exit(1);
  }

  // ── Command state ──
  let thinkingEnabled = false;
  let thinkingBudget: number | undefined;
  // Read model config from ~/.baoclaw/config.json
  let currentModel = "unknown";
  let fallbackModels: string[] = [];
  try {
    const raw = JSON.parse(fs.readFileSync(CONFIG_PATH, "utf-8"));
    currentModel = raw?.model || process.env.ANTHROPIC_MODEL || "unknown";
    fallbackModels = Array.isArray(raw?.fallback_models)
      ? raw.fallback_models
      : [];
  } catch {
    /* use defaults */
  }

  // ── Start Telegram bot ──
  const bot = new Bot(config.token);
  const sendMessage = (
    chatId: number,
    text: string,
    options?: Record<string, unknown>,
  ) => bot.api.sendMessage({ chat_id: chatId, text, ...options });
  const sendChatAction = (chatId: number, action: string) =>
    bot.api.sendChatAction({ chat_id: chatId, action });
  const sendPhoto = async (
    chatId: number,
    photo: string | InputFile,
    options?: Record<string, unknown>,
  ) =>
    bot.api.sendPhoto({
      chat_id: chatId,
      photo: typeof photo === "string" ? await fromPath(photo) : photo,
      ...options,
    });
  const sendDocument = async (
    chatId: number,
    document: string | InputFile,
    options?: Record<string, unknown>,
  ) =>
    bot.api.sendDocument({
      chat_id: chatId,
      document:
        typeof document === "string" ? await fromPath(document) : document,
      ...options,
    });
  const getFileLink = async (fileId: string): Promise<string> => {
    const file = await bot.api.getFile({ file_id: fileId });
    if (!file.file_path) throw new Error("Telegram returned no file path");
    return `https://api.telegram.org/file/bot${config.token}/${file.file_path}`;
  };

  let botInfo: User;
  try {
    botInfo = await bot.api.getMe();
    logger.info(`Telegram bot @${botInfo.username} ready.`);
  } catch (err: any) {
    logger.error(`Failed to connect to Telegram API: ${err.message}`);
    process.exit(1);
  }

  bot.catch((err: unknown) => {
    logger.error(
      `Telegram update error: ${err instanceof Error ? err.message : String(err)}`,
    );
  });

  // ── Write PID file ──
  const pidData = {
    pid: process.pid,
    bot_username: botInfo.username,
    daemon_pid: daemonInfo.pid,
    daemon_session_id: daemonInfo.session_id,
    started_at: new Date().toISOString(),
  };
  fs.mkdirSync(path.dirname(PID_FILE), { recursive: true });
  fs.writeFileSync(PID_FILE, JSON.stringify(pidData, null, 2));
  logger.info(`PID file: ${PID_FILE}`);

  // ── Per-chat state ──
  const chatQueue = new ChatQueue();
  // Per-chat response accumulator and completion signal
  const accumulators = new Map<number, string>();
  const thinkingAccumulators = new Map<number, string>();
  const resultResolvers = new Map<number, () => void>();
  // Per-chat pending attachments (for document/image uploads)
  const pendingAttachments = new Map<number, Record<string, unknown>[]>();
  let activeChatId: number | null = null;

  // ── Stream event handler ──
  ipcClient.onNotification("stream/event", async (params: unknown) => {
    const event = params as Record<string, unknown>;
    if (!event || typeof event !== "object") return;
    const chatId = activeChatId;
    if (chatId === null) return;

    switch (event.type) {
      case "assistant_chunk": {
        const content = (event as { content: string }).content || "";
        // If we were accumulating thinking, send it first
        const thinkingAcc = thinkingAccumulators.get(chatId);
        if (thinkingAcc && thinkingAcc.length > 0) {
          const thinkLen = Math.round(thinkingAcc.length / 4);
          const preview =
            thinkingAcc.length > 200
              ? thinkingAcc.slice(0, 200) + "…"
              : thinkingAcc;
          try {
            await sendMessage(
              chatId,
              `💭 <i>Thought (${thinkLen}tok)</i>\n<blockquote>${preview.replace(/</g, "&lt;").replace(/>/g, "&gt;")}</blockquote>`,
              { parse_mode: "HTML" },
            );
          } catch {}
          thinkingAccumulators.delete(chatId);
        }
        const current = accumulators.get(chatId) ?? "";
        accumulators.set(chatId, current + content);
        break;
      }

      case "thinking_chunk": {
        const content = (event as { content: string }).content || "";
        const current = thinkingAccumulators.get(chatId) ?? "";
        thinkingAccumulators.set(chatId, current + content);
        break;
      }

      case "tool_use": {
        const toolName =
          (event as { tool_name: string }).tool_name || "unknown";
        try {
          await sendMessage(chatId, `⚡ ${toolName}`);
        } catch {}
        break;
      }

      case "tool_result": {
        const tr = event as { is_error: boolean; output: unknown };
        if (tr.is_error) {
          const output =
            typeof tr.output === "string"
              ? tr.output
              : JSON.stringify(tr.output);
          const truncated =
            output.length > 500 ? output.slice(0, 500) + "..." : output;
          try {
            await sendMessage(chatId, `❌ Tool error: ${truncated}`);
          } catch {}
        } else {
          // Get output as string
          const outputStr =
            typeof tr.output === "string"
              ? tr.output
              : JSON.stringify(tr.output ?? "");

          // Helper: extract images from tool result content items
          // Supports multiple formats:
          //   MCP format:      { type: "image", data: "base64...", mimeType: "image/png" }
          //   Anthropic format: { type: "image", source: { type: "base64", media_type: "image/png", data: "base64..." } }
          //   Content array:   { content: [{ type: "image", ... }] }
          function extractImagesFromContent(
            content: any[],
          ): { buffer: Buffer; mediaType: string }[] {
            const imgs: { buffer: Buffer; mediaType: string }[] = [];
            for (const item of content) {
              if (item?.type !== "image") continue;
              // Anthropic format: data inside source
              if (
                item.source?.type === "base64" &&
                typeof item.source.data === "string" &&
                item.source.data.length > 100
              ) {
                const mediaType = item.source.media_type || "image/png";
                const ext = mediaType.split("/")[1] || "png";
                imgs.push({
                  buffer: Buffer.from(item.source.data, "base64"),
                  mediaType: ext,
                });
              }
              // MCP format: data at top level
              else if (
                typeof item.data === "string" &&
                item.data.length > 100
              ) {
                const ext =
                  (item.mimeType || item.media_type || "image/png").split(
                    "/",
                  )[1] || "png";
                imgs.push({
                  buffer: Buffer.from(item.data, "base64"),
                  mediaType: ext,
                });
              }
            }
            return imgs;
          }

          // Helper: send an image buffer via the Telegram photo adapter
          async function sendToolResultImage(
            chatId: number,
            img: { buffer: Buffer; mediaType: string },
            index: number,
            caption?: string,
          ): Promise<void> {
            const ext = img.mediaType === "jpeg" ? "jpg" : img.mediaType;
            const tmpFile = path.join(
              os.tmpdir(),
              `baoclaw-img-${Date.now()}-${index}.${ext}`,
            );
            fs.writeFileSync(tmpFile, img.buffer);
            const cap =
              caption ||
              (index === 0 ? "📸 图片已生成" : `📸 图片已生成 (${index + 1})`);
            await sendPhoto(chatId, tmpFile, { caption: cap });
            try {
              fs.unlinkSync(tmpFile);
            } catch {}
          }

          let sent = false;
          try {
            const parsed =
              typeof tr.output === "object" && tr.output !== null
                ? (tr.output as any)
                : JSON.parse(outputStr);

            // Case 1: Top-level image object (ImageGenTool format)
            // { type: "image", source: { type: "base64", media_type: "...", data: "..." } }
            if (
              parsed?.type === "image" &&
              parsed?.source?.data &&
              parsed.source.data.length > 100
            ) {
              const mediaType = parsed.source.media_type || "image/png";
              const ext = mediaType.split("/")[1] || "png";
              const buffer = Buffer.from(parsed.source.data, "base64");
              const caption = parsed.prompt
                ? `📸 ${parsed.prompt}`
                : "📸 图片已生成";
              await sendToolResultImage(
                chatId,
                { buffer, mediaType: ext },
                0,
                caption,
              );
              sent = true;
            }
            // Case 2: Content array format (MCP tools)
            // { content: [{ type: "image", data: "...", mimeType: "..." }] }
            else if (Array.isArray(parsed?.content)) {
              const images = extractImagesFromContent(parsed.content);
              for (let i = 0; i < images.length; i++) {
                try {
                  await sendToolResultImage(chatId, images[i], i);
                  sent = true;
                } catch (err) {
                  logger.error(`Failed to send tool result image: ${err}`);
                }
              }
            }
            // Case 3: Top-level MCP image (data at root)
            // { type: "image", data: "base64...", mimeType: "image/png" }
            else if (
              parsed?.type === "image" &&
              typeof parsed?.data === "string" &&
              parsed.data.length > 100
            ) {
              const ext =
                (parsed.mimeType || "image/png").split("/")[1] || "png";
              const buffer = Buffer.from(parsed.data, "base64");
              await sendToolResultImage(chatId, { buffer, mediaType: ext }, 0);
              sent = true;
            }
          } catch {
            // JSON parse failed (likely truncated output) — extract base64 with regex
            const b64Match = outputStr.match(
              /"data"\s*:\s*"([A-Za-z0-9+/=]{1000,})"/,
            );
            if (b64Match) {
              try {
                const tmpFile = path.join(
                  os.tmpdir(),
                  `baoclaw-img-${Date.now()}.png`,
                );
                fs.writeFileSync(tmpFile, Buffer.from(b64Match[1], "base64"));
                await sendPhoto(chatId, tmpFile, { caption: "📸 图片已生成" });
                try {
                  fs.unlinkSync(tmpFile);
                } catch {}
                sent = true;
              } catch (err) {
                logger.error(
                  `Failed to extract/send image from truncated output: ${err}`,
                );
              }
            }
          }
          if (sent) {
            // Don't send redundant text message — the photo was already sent
          }
        }
        break;
      }

      case "error": {
        const err = event as { code: string; message: string };
        try {
          await sendMessage(
            chatId,
            `❌ [${err.code || "ERROR"}] ${err.message || "Unknown error"}`,
          );
        } catch {}
        // Signal completion
        const resolver = resultResolvers.get(chatId);
        if (resolver) {
          resultResolvers.delete(chatId);
          resolver();
        }
        break;
      }

      case "result": {
        const accumulated = accumulators.get(chatId) ?? "";
        if (accumulated.length > 0) {
          // Extract and send base64 images as real photos
          const { text, images } = extractBase64Images(accumulated);
          if (images.length > 0) {
            logger.info(
              `Extracted ${images.length} image(s) from accumulated text (${accumulated.length} chars)`,
            );
          }
          // Send text first
          if (text.trim().length > 0) {
            const chunks = splitMessage(text, MAX_TG_MSG);
            for (const chunk of chunks) {
              try {
                await sendMessage(chatId, markdownToTelegramHtml(chunk), {
                  parse_mode: "HTML",
                });
              } catch {
                try {
                  await sendMessage(chatId, chunk);
                } catch (err) {
                  logger.error(`Failed to send Telegram message: ${err}`);
                }
              }
            }
          }
          // Then send images
          for (const img of images) {
            try {
              const tmpFile = path.join(
                os.tmpdir(),
                `baoclaw-img-${Date.now()}-${Math.random().toString(36).slice(2, 6)}.png`,
              );
              fs.writeFileSync(tmpFile, img.buffer);
              await sendPhoto(chatId, tmpFile, {
                caption: img.caption || undefined,
              });
              fs.unlinkSync(tmpFile);
            } catch (err) {
              logger.error(
                `Failed to send photo (${img.buffer.length} bytes): ${err}`,
              );
            }
          }
        }
        accumulators.delete(chatId);
        thinkingAccumulators.delete(chatId);
        // Signal completion
        const resolver = resultResolvers.get(chatId);
        if (resolver) {
          resultResolvers.delete(chatId);
          resolver();
        }
        break;
      }
    }
  });

  // ── Handle daemon disconnect ──
  ipcClient.onDisconnect(() => {
    logger.warn("Daemon connection lost. Shutting down.");
    bot.stop();
    try {
      fs.unlinkSync(PID_FILE);
    } catch {}
    process.exit(1);
  });

  // ── Command handler functions ──
  // Each handler checks connection, calls IPC, formats result, wraps in try/catch.

  async function handleTools(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{ tools: any[]; count: number }>(
        "listTools",
      );
      return formatTools(result.tools, result.count);
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleSkills(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{ skills: any[]; count: number }>(
        "listSkills",
      );
      return formatSkills(result.skills, result.count);
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleMcp(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{ servers: any[]; count: number }>(
        "listMcpServers",
      );
      return formatMcpServers(result.servers, result.count);
    } catch (err) {
      return formatError(err);
    }
  }

  async function handlePlugins(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{ plugins: any[]; count: number }>(
        "listPlugins",
      );
      return formatPlugins(result.plugins, result.count);
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleCompact(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{
        tokens_saved: number;
        summary_tokens: number;
        tokens_before: number;
        tokens_after: number;
      }>("compact");
      return formatCompact(result);
    } catch (err: any) {
      const msg = err?.message || "";
      if (msg.includes("session busy") || msg.includes("mutate busy")) {
        return "⏳ 会话正忙，无法执行此操作。";
      }
      return formatError(err);
    }
  }

  async function handleThink(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      thinkingEnabled = !thinkingEnabled;
      const settings = thinkingEnabled
        ? {
            thinking: {
              type: "enabled",
              budget_tokens: thinkingBudget ?? 10000,
            },
          }
        : { thinking: { type: "disabled" } };
      await ipcClient.request("updateSettings", { settings });
      return formatThinkToggle(
        thinkingEnabled,
        thinkingEnabled ? (thinkingBudget ?? 10000) : undefined,
      );
    } catch (err) {
      thinkingEnabled = !thinkingEnabled; // revert on failure
      return formatError(err);
    }
  }

  async function handleModel(args: string): Promise<string> {
    if (!args) {
      return formatModelInfo(currentModel, fallbackModels);
    }
    if (!ipcClient.connected) return formatDisconnected();
    try {
      await ipcClient.request("switchModel", { model: args });
      return formatModelSwitch(args);
    } catch (err: any) {
      const msg = err?.message || "";
      if (msg.includes("session busy") || msg.includes("mutate busy")) {
        return "⏳ 会话正忙，无法执行此操作。";
      }
      return formatError(err);
    }
  }

  async function handleDiff(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{ diff: string }>("gitDiff");
      return formatGitDiff(result);
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleCommit(args: string): Promise<string> {
    if (!args) return formatCommitUsage();
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{ hash: string; message: string }>(
        "gitCommit",
        { message: args },
      );
      return formatGitCommit(result);
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleGit(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<any>("gitStatus");
      return formatGitStatus(result);
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleAbort(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      await ipcClient.request("abort");
      return formatAbortConfirm();
    } catch (err) {
      return formatError(err);
    }
  }

  function handleHelp(): string {
    return formatHelp(COMMAND_REGISTRY);
  }

  function handleStatus(): string {
    return formatStatus(daemonInfo, botInfo.username!, sessionState, {
      reconnectCount: daemonConnector.reconnectCount,
      lastConnectAt: daemonConnector.lastConnectAt,
    });
  }

  function handleStart(chatId: number): string {
    return formatStart(daemonInfo, chatId, sessionState);
  }

  function handleClear(): string {
    return (
      `ℹ️ Each Telegram connection has its own conversation history managed by the daemon. ` +
      `Reconnect the gateway for a fresh session.`
    );
  }

  async function handleShutdown(): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      await ipcClient.request("shutdown");
      // Daemon will exit, which triggers our onDisconnect handler
      return "🛑 Daemon 正在关闭...";
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleQuit(chatId: number): Promise<string> {
    // Send goodbye, then shut down the gateway process
    setTimeout(() => {
      logger.info("Quit requested via Telegram");
      bot.stop();
      ipcClient.disconnect().catch(() => {});
      try {
        fs.unlinkSync(PID_FILE);
      } catch {}
      process.exit(0);
    }, 500);
    return "👋 Telegram Gateway 正在断开...（Daemon 保持运行）";
  }

  async function handleMemory(args: string): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    const parts = args.split(/\s+/);
    const subCmd = parts[0] || "";
    const rest = parts.slice(1).join(" ");

    try {
      if (subCmd === "list" || subCmd === "ls") {
        const result = await ipcClient.request<{
          memories: any[];
          count: number;
        }>("memoryList");
        if (result.count === 0) return "暂无长期记忆。";
        let out = `🧠 长期记忆 (${result.count})\n\n`;
        for (const m of result.memories) {
          out += `• [${m.id}] [${m.category}] ${m.content}\n`;
        }
        return out;
      } else if (subCmd === "add") {
        let category = "fact";
        let content = rest;
        if (
          parts[1] &&
          ["fact", "preference", "pref", "decision", "dec"].includes(parts[1])
        ) {
          category = parts[1];
          content = parts.slice(2).join(" ");
        }
        if (!content)
          return "用法: /memory add [fact|preference|decision] <内容>";
        const result = await ipcClient.request<{ memory: any }>("memoryAdd", {
          content,
          category,
        });
        return `✅ 记忆已添加 [${result.memory.id}] ${result.memory.content}`;
      } else if (subCmd === "delete" || subCmd === "del" || subCmd === "rm") {
        if (!rest) return "用法: /memory delete <id>";
        const result = await ipcClient.request<{ deleted: boolean }>(
          "memoryDelete",
          { id: rest },
        );
        return result.deleted ? "✅ 记忆已删除" : `❌ 未找到记忆: ${rest}`;
      } else if (subCmd === "clear") {
        const result = await ipcClient.request<{ cleared: number }>(
          "memoryClear",
        );
        return `✅ 已清除 ${result.cleared} 条记忆`;
      } else {
        return "🧠 记忆命令\n\n/memory list — 列出所有记忆\n/memory add [分类] <内容> — 添加记忆\n/memory delete <id> — 删除记忆\n/memory clear — 清除所有记忆";
      }
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleHistory(args: string): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    const count = parseInt(args, 10) || 10;
    try {
      const result = await ipcClient.request<{
        messages: any[];
        count: number;
        total: number;
      }>("talkTail", { count });
      if (result.count === 0) return "暂无对话记录。";
      let out = `📜 最近对话 (${result.count}/${result.total})\n\n`;
      for (const m of result.messages) {
        const ts = m.timestamp ? m.timestamp.slice(11, 19) : "";
        if (m.role === "user") {
          const text = (m.text || "").slice(0, 80);
          out += `${ts}  👤 ${text}${text.length >= 80 ? "…" : ""}\n`;
        } else if (m.role === "assistant") {
          const text = (m.text || "").slice(0, 80);
          const tools =
            m.tools && m.tools.length > 0 ? ` [${m.tools.length}🔧]` : "";
          out += `${ts}  🤖${tools} ${text}${text.length >= 80 ? "…" : ""}\n`;
        }
      }
      return out;
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleExport(chatId: number): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{
        messages: any[];
        count: number;
        total: number;
      }>("talkTail", { count: 9999 });
      if (result.count === 0) return "当前会话无对话记录";

      const entries = result.messages.map((m: any) => ({
        role: m.role as "user" | "assistant",
        text: m.text || "",
        timestamp: m.timestamp,
        tools: m.tools,
      }));

      const markdown = formatTranscriptToMarkdown(entries, {
        sessionId: sessionState.sessionId,
      });
      const filename = defaultExportFilename();
      const filepath = path.join(os.tmpdir(), filename);
      fs.writeFileSync(filepath, markdown, "utf-8");

      try {
        await sendDocument(chatId, filepath, { caption: "📄 对话导出" });
      } finally {
        try {
          fs.unlinkSync(filepath);
        } catch {}
      }

      return "";
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleSearch(args: string): Promise<string> {
    if (!args.trim()) return "用法: /search <关键词>";
    if (!ipcClient.connected) return formatDisconnected();
    try {
      const result = await ipcClient.request<{ results: SearchResult[] }>(
        "searchHistory",
        { query: args.trim(), max_results: 10 },
      );
      return formatSearchResults(result.results || [], args.trim());
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleSpec(args: string): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    const parts = args.split(/\s+/);
    const subCmd = parts[0] || "list";
    const featureName = parts[1] || "";

    try {
      if (subCmd === "list") {
        const result = await ipcClient.request<{ specs: any[] }>("specList");
        const specs = result.specs || [];
        if (specs.length === 0)
          return "暂无 Spec。使用 /spec new <feature-name> 创建。";
        let out = `📋 Specs (${specs.length})\n\n`;
        for (const s of specs) {
          const progress = s.task_progress
            ? ` [${s.task_progress.completed}/${s.task_progress.total}]`
            : "";
          out += `• ${s.feature_name}  ${s.phase}${progress}\n`;
        }
        return out;
      } else if (subCmd === "new") {
        if (!featureName)
          return "用法: /spec new <feature-name> [requirements|design]";
        const workflow = parts[2] || "requirements";
        const result = await ipcClient.request<any>("specNew", {
          feature_name: featureName,
          workflow,
        });
        return `✅ Spec "${featureName}" 已创建 (${workflow})`;
      } else if (subCmd === "show") {
        if (!featureName) return "用法: /spec show <feature-name>";
        const result = await ipcClient.request<any>("specShow", {
          feature_name: featureName,
        });
        const progress = result.task_progress
          ? `\n进度: ${result.task_progress.completed}/${result.task_progress.total}`
          : "";
        return `📄 ${result.feature_name}\n阶段: ${result.phase}\n类型: ${result.spec_type}${progress}`;
      } else if (subCmd === "status") {
        if (!featureName) return "用法: /spec status <feature-name>";
        const result = await ipcClient.request<any>("specStatus", {
          feature_name: featureName,
        });
        return `📊 ${featureName}\n总计: ${result.total} | 完成: ${result.completed} | 进行中: ${result.in_progress}`;
      } else if (subCmd === "run") {
        if (!featureName) return "用法: /spec run <feature-name> [task-id]";
        const taskId = parts[2] || undefined;
        const result = await ipcClient.request<any>("specRun", {
          feature_name: featureName,
          task_id: taskId,
        });
        if (result.status === "all_complete") return "✅ 所有任务已完成";
        return `▶️ 准备执行: [${result.task_id}] ${result.task_description}`;
      } else if (subCmd === "edit") {
        if (!featureName)
          return "用法: /spec edit <feature-name> [requirements|design|tasks]";
        const phase = parts[2] || "requirements";
        const result = await ipcClient.request<any>("specEdit", {
          feature_name: featureName,
          phase,
        });
        const content = result.content || "";
        if (content.length > 4000) {
          return content.slice(0, 4000) + "\n\n...[内容过长，已截断]";
        }
        return content;
      } else {
        return "用法: /spec [list|new|show|status|run|edit] <feature-name>";
      }
    } catch (err) {
      return formatError(err);
    }
  }

  // Command handler dispatch table
  async function handleCron(args: string): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    const parts = args.split(/\s+/);
    const subCmd = parts[0] || "";

    try {
      if (subCmd === "list" || subCmd === "") {
        const result = await ipcClient.request<{ jobs: any[]; count: number }>(
          "cronList",
        );
        if (result.count === 0) return "暂无定时任务。使用 /cron add 创建。";
        let out = `⏰ 定时任务 (${result.count})\n\n`;
        for (const j of result.jobs) {
          const status = j.enabled ? "✅" : "⏸️";
          const last = j.last_run ? j.last_run.slice(0, 19) : "未运行";
          const prompt =
            j.prompt.length > 50 ? j.prompt.slice(0, 50) + "…" : j.prompt;
          out += `${status} [${j.id}] ${j.name}  ${j.schedule}\n`;
          out += `  ${last}  ${prompt}\n\n`;
        }
        return out;
      } else if (subCmd === "add") {
        const match = args.match(/add\s+"([^"]+)"\s+"([^"]+)"\s+(.+)/);
        if (!match)
          return '用法: /cron add "任务名" "every 1h" 提示词\n\n支持: every 30m, daily 09:00, weekly mon 09:00';
        const result = await ipcClient.request<{ job: any }>("cronAdd", {
          name: match[1],
          schedule: match[2],
          prompt: match[3],
        });
        return `✅ 定时任务已创建 [${result.job.id}] ${result.job.name} (${result.job.schedule})`;
      } else if (subCmd === "remove" || subCmd === "rm") {
        const jobId = parts[1];
        if (!jobId) return "用法: /cron remove <id>";
        const result = await ipcClient.request<{ removed: boolean }>(
          "cronRemove",
          { id: jobId },
        );
        return result.removed ? "✅ 已删除" : "❌ 未找到该任务";
      } else if (subCmd === "toggle") {
        const jobId = parts[1];
        if (!jobId) return "用法: /cron toggle <id>";
        const result = await ipcClient.request<{ enabled: boolean }>(
          "cronToggle",
          { id: jobId },
        );
        return result.enabled ? "✅ 已启用" : "⏸️ 已禁用";
      } else {
        return '⏰ 定时任务命令\n\n/cron list — 列出所有任务\n/cron add "名称" "计划" 提示词\n/cron remove <id>\n/cron toggle <id>';
      }
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleProjects(args: string): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    const parts = args.split(/\s+/);
    const subCmd = parts[0] || "";

    try {
      if (subCmd === "list" || subCmd === "") {
        const result = await ipcClient.request<{
          projects: any[];
          count: number;
        }>("projectsList");
        if (result.count === 0)
          return "暂无项目。使用 /projects new <路径> [描述] 创建。";
        let out = `📂 项目列表 (${result.count})\n\n`;
        for (const p of result.projects) {
          const last = p.last_accessed ? p.last_accessed.slice(0, 10) : "";
          const sid = p.session_id ? `  session:${p.session_id}` : "";
          out += `[${p.id}] ${p.description}${last ? "  (" + last + ")" : ""}${sid}\n`;
          out += `  ${p.cwd}\n\n`;
        }
        out += "切换: /projects <id>  ·  新建: /projects new <路径> [描述]";
        return out;
      } else if (subCmd === "new") {
        const rest = args.slice(3).trim();
        const spaceIdx = rest.indexOf(" ");
        let targetPath: string;
        let desc: string | undefined;
        if (spaceIdx > 0) {
          targetPath = rest.slice(0, spaceIdx);
          desc = rest.slice(spaceIdx + 1).trim() || undefined;
        } else {
          targetPath = rest;
        }
        if (!targetPath) return "用法: /projects new <路径> [描述]";
        const params: Record<string, unknown> = { cwd: targetPath };
        if (desc) params.description = desc;
        const result = await ipcClient.request<{ project: any }>(
          "projectsNew",
          params,
        );
        return `✅ 已创建并切换到: ${result.project.description}\n  [${result.project.id}] ${result.project.cwd}`;
      } else {
        // /projects <id_prefix> — switch
        const result = await ipcClient.request<{
          project: any;
          message_count: number;
        }>("projectsSwitch", { id_prefix: subCmd });
        let msg = `📂 已切换到: ${result.project.description}\n  [${result.project.id}] ${result.project.cwd}`;
        if (result.message_count > 0)
          msg += `\n  已恢复 ${result.message_count} 条消息`;
        return msg;
      }
    } catch (err) {
      return formatError(err);
    }
  }

  async function handleTask(args: string): Promise<string> {
    if (!ipcClient.connected) return formatDisconnected();
    const parts = args.split(/\s+/);
    const subCmd = parts[0] || "";

    try {
      if (subCmd === "run") {
        const desc = args
          .slice(3)
          .trim()
          .replace(/^["']|["']$/g, "");
        if (!desc) return '用法: /task run "任务描述"';
        const result = await ipcClient.request<{ task_id: string }>(
          "taskCreate",
          { description: desc, prompt: desc },
        );
        return `✅ 后台任务已创建 [${result.task_id}]`;
      } else if (subCmd === "list" || subCmd === "") {
        const result = await ipcClient.request<{ tasks: any[]; count: number }>(
          "taskList",
        );
        if (result.count === 0) return "暂无后台任务。";
        let out = `📋 后台任务 (${result.count})\n\n`;
        for (const t of result.tasks) {
          const status =
            typeof t.status === "string" ? t.status : JSON.stringify(t.status);
          out += `[${t.id}] ${status} ${t.description}\n`;
        }
        return out;
      } else if (subCmd === "status") {
        const taskId = parts[1];
        if (!taskId) return "用法: /task status <id>";
        const t = await ipcClient.request<any>("taskStatus", {
          task_id: taskId,
        });
        return `📋 任务 ${t.id}\n状态: ${typeof t.status === "string" ? t.status : JSON.stringify(t.status)}\n描述: ${t.description}`;
      } else if (subCmd === "stop") {
        const taskId = parts[1];
        if (!taskId) return "用法: /task stop <id>";
        const result = await ipcClient.request<{ stopped: boolean }>(
          "taskStop",
          { task_id: taskId },
        );
        return result.stopped ? "✅ 已停止" : "❌ 未找到或未在运行";
      } else {
        return '📋 后台任务命令\n\n/task run "描述" — 创建任务\n/task list — 列出任务\n/task status <id> — 查看状态\n/task stop <id> — 停止任务';
      }
    } catch (err) {
      return formatError(err);
    }
  }

  // Command handler dispatch table
  const commandHandlers: Record<
    string,
    (args: string, chatId: number) => Promise<string> | string
  > = {
    "/tools": (args) => handleTools(),
    "/skills": (args) => handleSkills(),
    "/mcp": (args) => handleMcp(),
    "/plugins": (args) => handlePlugins(),
    "/compact": (args) => handleCompact(),
    "/think": (args) => handleThink(),
    "/model": (args) => handleModel(args),
    "/diff": (args) => handleDiff(),
    "/commit": (args) => handleCommit(args),
    "/git": (args) => handleGit(),
    "/abort": (args) => handleAbort(),
    "/help": () => handleHelp(),
    "/status": () => handleStatus(),
    "/start": (_args, chatId) => handleStart(chatId),
    "/clear": () => handleClear(),
    "/shutdown": () => handleShutdown(),
    "/quit": (_args, chatId) => handleQuit(chatId),
    "/memory": (args) => handleMemory(args),
    "/cron": (args) => handleCron(args),
    "/projects": (args) => handleProjects(args),
    "/task": (args) => handleTask(args),
    "/history": (args) => handleHistory(args),
    "/export": async (_args, chatId) => handleExport(chatId),
    "/search": (args) => handleSearch(args),
    "/spec": (args) => handleSpec(args),
  };

  // ── Process a single message for a chat ──
  async function processMessage(
    chatId: number,
    text: string,
    attachments?: Record<string, unknown>[],
  ): Promise<void> {
    activeChatId = chatId;
    accumulators.set(chatId, "");

    // Create a promise that resolves when result/error event arrives
    const resultPromise = new Promise<void>((resolve) => {
      resultResolvers.set(chatId, resolve);
    });

    try {
      await sendChatAction(chatId, "typing");
      const params: Record<string, unknown> = { prompt: text };
      if (attachments && attachments.length > 0) {
        params.attachments = attachments;
      }
      await ipcClient.request("submitMessage", params);
      // Wait for the stream to complete (result or error event)
      await resultPromise;
    } catch (err: any) {
      const msg = err.message || "";
      if (msg.includes("session busy")) {
        // -32001: another client is submitting a message
        try {
          await sendMessage(
            chatId,
            "⏳ 会话正忙，另一个客户端正在提交消息，请稍后再试。",
          );
        } catch {}
      } else {
        logger.error(`submitMessage error for chat ${chatId}: ${msg}`);
        try {
          await sendMessage(chatId, `❌ ${msg}`);
        } catch {}
      }
      // Clean up in case result never came
      accumulators.delete(chatId);
      thinkingAccumulators.delete(chatId);
      resultResolvers.delete(chatId);
    }

    activeChatId = null;
  }

  // ── Process queue for a chat ──
  async function processQueue(chatId: number): Promise<void> {
    chatQueue.startProcessing(chatId);
    while (chatQueue.hasQueued(chatId)) {
      const text = chatQueue.dequeue(chatId);
      if (!text) break;
      // Check for pending attachments
      const attachments = pendingAttachments.get(chatId);
      pendingAttachments.delete(chatId);
      await processMessage(chatId, text, attachments);
    }
    chatQueue.finishProcessing(chatId);
  }

  // ── Bot message handler ──
  bot.on("message", async (ctx: Context) => {
    const msg = ctx.message;
    if (!msg) return;
    const chatId = msg.chat.id;

    // Allowlist is validated at startup; reject every non-member.
    if (!isAllowedChat(chatId, config.allowedChatIds)) {
      logger.info(`Rejected: chat ${chatId}`);
      return;
    }

    // ── Handle document uploads (PDF, DOCX) ──
    if (msg.document) {
      const doc = msg.document;
      const fileName = doc.file_name || "unknown";
      const mimeType = doc.mime_type || "application/octet-stream";
      const caption = msg.caption || `请分析这个文件: ${fileName}`;

      try {
        await sendMessage(chatId, `📄 正在处理文件: ${fileName}...`);
        const fileLink = await getFileLink(doc.file_id);
        const resp = await fetch(fileLink);
        const buffer = Buffer.from(await resp.arrayBuffer());

        // Route B: try native document block (PDF only)
        const docBlock = buildDocumentBlock(buffer, mimeType);
        if (docBlock) {
          // Send as attachment for native API support
          chatQueue.enqueue(chatId, caption);
          // Store attachments for the next processMessage call
          pendingAttachments.set(chatId, [docBlock]);
          if (!chatQueue.isProcessing(chatId)) {
            processQueue(chatId);
          }
          return;
        }

        // Route A: extract text for non-PDF or as fallback
        const parsed = await parseDocument(buffer, mimeType, fileName);
        if (parsed.error) {
          await sendMessage(chatId, `❌ ${parsed.error}`);
          return;
        }
        if (!parsed.text.trim()) {
          await sendMessage(chatId, "⚠️ 文件内容为空或无法提取文本。");
          return;
        }

        // Truncate if too large (keep ~100k chars to stay within context limits)
        const maxChars = 100_000;
        let docText = parsed.text;
        if (docText.length > maxChars) {
          docText =
            docText.slice(0, maxChars) +
            `\n\n[... 文档已截断，共 ${parsed.text.length} 字符]`;
        }

        const prompt = `[文件: ${fileName}${parsed.pageCount ? ` (${parsed.pageCount}页)` : ""}]\n\n${docText}\n\n---\n${caption}`;
        chatQueue.enqueue(chatId, prompt);
        if (!chatQueue.isProcessing(chatId)) {
          processQueue(chatId);
        }
      } catch (err: any) {
        logger.error(`Document processing error: ${err.message}`);
        try {
          await sendMessage(chatId, `❌ 文件处理失败: ${err.message}`);
        } catch {}
      }
      return;
    }

    // ── Handle photo uploads ──
    if (msg.photo && msg.photo.length > 0) {
      const photo = msg.photo[msg.photo.length - 1]; // highest resolution
      const caption = msg.caption || "请描述这张图片";

      try {
        await sendMessage(chatId, "🖼️ 正在处理图片...");
        const fileLink = await getFileLink(photo.file_id);
        const resp = await fetch(fileLink);
        const buffer = Buffer.from(await resp.arrayBuffer());

        // Detect mime type from file extension
        const ext = fileLink.split(".").pop()?.toLowerCase() || "jpg";
        const mimeMap: Record<string, string> = {
          jpg: "image/jpeg",
          jpeg: "image/jpeg",
          png: "image/png",
          gif: "image/gif",
          webp: "image/webp",
        };
        const mimeType = mimeMap[ext] || "image/jpeg";

        const imageBlock = buildImageBlock(buffer, mimeType);
        chatQueue.enqueue(chatId, caption);
        pendingAttachments.set(chatId, [imageBlock]);
        if (!chatQueue.isProcessing(chatId)) {
          processQueue(chatId);
        }
      } catch (err: any) {
        logger.error(`Photo processing error: ${err.message}`);
        try {
          await sendMessage(chatId, `❌ 图片处理失败: ${err.message}`);
        } catch {}
      }
      return;
    }

    // ── Handle text messages ──
    if (!msg.text) return;

    // Command routing
    const parsed = parseCommand(msg.text);
    if (parsed && isRegisteredCommand(msg.text)) {
      const handler = commandHandlers[parsed.command];
      if (handler) {
        try {
          const result = await handler(parsed.args, chatId);
          if (result) {
            const chunks = splitMessage(result, MAX_TG_MSG);
            for (const chunk of chunks) {
              await sendMessage(chatId, chunk);
            }
          }
        } catch (err) {
          await sendMessage(chatId, formatError(err));
        }
        return;
      }
    }

    // Unregistered commands and regular messages → enqueue for AI
    chatQueue.enqueue(chatId, msg.text);
    if (!chatQueue.isProcessing(chatId)) {
      processQueue(chatId);
    }
  });

  // ── Graceful shutdown ──
  const shutdown = (signal: string) => {
    logger.info(`Shutdown (${signal})`);
    bot.stop();
    ipcClient.disconnect().catch(() => {});
    try {
      fs.unlinkSync(PID_FILE);
    } catch {}
    process.exit(0);
  };
  process.on("SIGTERM", () => shutdown("SIGTERM"));
  process.on("SIGINT", () => shutdown("SIGINT"));

  logger.info("Telegram Gateway ready.");
  await run(bot);
}

main().catch((err) => {
  logger.error(`Gateway failed: ${err.message}`);
  process.exit(1);
});
