/**
 * WhatsApp Gateway — main process.
 * Orchestrates: load config → Baileys init → daemon discover → message loop.
 * Handles inbound WhatsApp messages, outbound daemon responses,
 * graceful shutdown, and PID file management.
 *
 * Integrates: SenderTracker, PermissionManager, Commands, MediaHandler.
 *
 * Note: Hardware AES-256-CBC decrypt is broken on this host's CPU (Zhaoxin
 * KaiXian KX-7000 / VIA-derived family). Baileys and libsignal sources are
 * patched (via patch-package) to route through `_aes_cbc_shim.js`, which
 * delegates to a pure-JS AES implementation. See `_aes_cbc_shim.js` and
 * `src/cryptoPatch.ts`.
 */
import * as fs from "fs";
import * as path from "path";
import * as os from "os";
import {
  loadWhatsAppConfig,
  validateAllowFrom,
  watchConfig,
  type WhatsAppConfig,
} from "./config.js";
import { isAllowed, validateE164, normalizeJid } from "./allowlist.js";
import { RateLimiter } from "./rateLimiter.js";
import {
  formatForWhatsApp,
  splitMessage,
  formatToolUse,
  formatError,
} from "./formatter.js";
import { MessageQueue } from "./messageQueue.js";
import { createDaemonConnector, type DaemonInfo } from "./daemon.js";
import { IpcClient } from "../../ts-ipc/index.js";
import { createLogger } from "../../ts-ipc/logger.js";
import { SessionManager } from "./session.js";
// New modules
import { SenderTracker } from "./senderTracker.js";
import { PermissionManager } from "./permission.js";
import {
  parseCommand,
  isRegisteredCommand,
  dispatchCommand,
  formatHelp,
  COMMAND_REGISTRY,
  setGatewayInfo,
  setDaemonMetrics,
} from "./commands.js";
import { MediaHandler, isImageFile } from "./media.js";

const logger = createLogger("whatsapp");

const PID_FILE = path.join(os.homedir(), ".baoclaw", "whatsapp-gateway.pid");
const SHUTDOWN_TIMEOUT_MS = 10_000;

export interface GatewayOptions {
  configPath?: string;
}

export class WhatsAppGateway {
  private config!: WhatsAppConfig;
  private configWatcher: fs.FSWatcher | null = null;
  private session: SessionManager;
  private daemonConnector: ReturnType<typeof createDaemonConnector>;
  private ipcClient: IpcClient | null = null;
  private daemonInfo: DaemonInfo | null = null;
  private rateLimiter: RateLimiter;
  private messageQueue: MessageQueue;
  private shuttingDown = false;
  private configPath: string;

  // New module instances
  private senderTracker: SenderTracker;
  private permissionManager: PermissionManager;
  private mediaHandler: MediaHandler;

  // Active sender for stream handler (simplification for single-active-sender model)
  private activeSender: string | null = null;
  // Processing flags: signals processQueue when result/error is complete
  private processingFlags = new Set<string>();

  constructor(options?: GatewayOptions) {
    this.configPath =
      options?.configPath ?? path.join(os.homedir(), ".baoclaw", "config.json");
    this.session = new SessionManager(undefined, undefined); // phone set after config load
    this.daemonConnector = createDaemonConnector();
    this.rateLimiter = new RateLimiter();
    this.messageQueue = new MessageQueue();
    this.senderTracker = new SenderTracker();
    this.permissionManager = new PermissionManager(this.senderTracker);
    this.mediaHandler = new MediaHandler();
  }

  /**
   * 4.1 — Start the gateway: load config → Baileys init → daemon discover → message loop.
   */
  async start(): Promise<void> {
    logger.info("WhatsApp Gateway starting...");

    // Load config
    this.config = loadWhatsAppConfig(this.configPath);

    if (!this.config.enabled) {
      logger.info(
        "WhatsApp is disabled in configuration (whatsapp.enabled = false). Exiting.",
      );
      process.exit(0);
    }

    // Validate allowlist entries
    const validAllow = validateAllowFrom(this.config);
    for (const entry of this.config.allowFrom) {
      if (!validateE164(entry)) {
        logger.warn(`Invalid E.164 number in allowFrom, skipping: ${entry}`);
      }
    }
    this.config.allowFrom = validAllow;

    if (validAllow.length === 0) {
      throw new Error(
        "Cannot start WhatsApp gateway because allowFrom is empty or invalid. To fix, set at least one E.164 number in config.json.",
      );
    }

    // Watch config for hot-reload
    this.configWatcher = watchConfig(this.configPath, (newConfig) => {
      const validAllow = validateAllowFrom(newConfig);
      if (validAllow.length === 0) {
        logger.warn(
          "Ignoring config reload because allowFrom is empty or invalid.",
        );
        return;
      }
      logger.info("Config reloaded.");
      this.config = { ...newConfig, allowFrom: validAllow };
    });

    // Initialize Baileys session with phone number from config
    this.session = new SessionManager(
      undefined,
      this.config.phoneNumber ?? undefined,
      this.config.proxy ?? undefined,
    );
    logger.info("Initializing WhatsApp connection...");
    const sock = await this.session.initialize();

    // Discover and connect to daemon (with sharedSessionId)
    logger.info("Discovering BaoClaw daemon...");
    const { client, info } = await this.daemonConnector.discoverAndConnect(
      60_000,
      5_000,
      this.config.sharedSessionId,
    );
    this.ipcClient = client;
    this.daemonInfo = info;
    setDaemonMetrics({
      reconnectCount: this.daemonConnector.reconnectCount,
      lastConnectAt: this.daemonConnector.lastConnectAt,
    });
    logger.info(
      `Connected to daemon pid=${info.pid} session=${info.session_id}`,
    );

    // Set gateway info for /gateway management command
    setGatewayInfo({
      pid: process.pid,
      startTime: Date.now(),
      logFile: path.join(
        os.homedir(),
        ".baoclaw",
        "logs",
        "baoclaw-whatsapp.log",
      ),
      name: "WhatsApp",
    });

    // Print registered commands
    logger.info(
      `Registered ${COMMAND_REGISTRY ? Object.keys(COMMAND_REGISTRY).length : 0} commands.`,
    );

    // Set up daemon stream event handler (4.3 — outbound)
    this.setupStreamHandler(sock);

    // Set up daemon disconnect handler
    client.onDisconnect(() => {
      if (!this.shuttingDown) {
        logger.warn("Daemon connection lost. Attempting reconnect...");
        this.reconnectDaemon(sock, 1);
      }
    });

    // Set up inbound message handler (4.2)
    this.setupInboundHandler(sock);

    // Write PID file (4.5)
    this.writePidFile();

    // Set up graceful shutdown (4.4)
    this.setupShutdownHandlers(sock);

    logger.info("WhatsApp Gateway is ready.");
  }

  /**
   * 4.2 — Inbound: WhatsApp msg → dedup → allowlist → rate limit → queue → submitMessage RPC.
   * Handles text, image, document, permission replies, and commands.
   */
  private setupInboundHandler(sock: any): void {
    sock.ev.on("messages.upsert", async (m: any) => {
      if (this.shuttingDown) return;
      const messages = m.messages || [];

      for (const msg of messages) {
        // Dedup check
        const msgId = msg.key?.id;
        if (msgId && this.messageQueue.isDuplicate(msgId)) continue;

        // Skip broadcasts only (allow fromMe for self-testing)
        if (msg.key.remoteJid === "status@broadcast") continue;

        // Baileys 7+ uses LID addressing by default. The real phone-number JID
        // is in `remoteJidAlt` (for DMs) or `participantAlt` (for groups).
        const rawJid = msg.key.remoteJid!;
        // Use rawJid (LID) for sending replies — Baileys requires it.
        const replyJid = rawJid;
        const isGroup = rawJid.endsWith("@g.us");

        // Policy check
        if (isGroup && this.config.groupPolicy === "ignore") continue;
        if (!isGroup && this.config.dmPolicy === "ignore") continue;

        // Determine sender phone — prefer phone-number JID over LID for allowlist matching
        const senderJid = isGroup
          ? (msg.key as any).participantAlt || msg.key.participant || rawJid
          : (msg.key as any).remoteJidAlt || rawJid;
        const senderPhone = normalizeJid(senderJid);

        // Trace inbound for diagnostics
        const previewText =
          msg.message?.conversation ??
          msg.message?.extendedTextMessage?.text ??
          "";
        logger.info(
          `📥 inbound: replyJid=${replyJid} senderJid=${senderJid} senderPhone=${senderPhone} group=${isGroup} text="${String(previewText).slice(0, 60)}"`,
        );

        // Allowlist check
        if (!isAllowed(senderPhone, this.config.allowFrom)) {
          logger.info(
            `  ↳ rejected by allowlist (allowFrom=${JSON.stringify(this.config.allowFrom)})`,
          );
          continue;
        }

        // Rate limit check
        if (!this.rateLimiter.tryConsume(senderPhone)) {
          logger.info(`Rate limited: ${senderPhone}`);
          try {
            await sock.sendMessage(replyJid, {
              text: "⏳ Rate limit exceeded. Please wait.",
            });
          } catch {}
          continue;
        }

        // Register sender → JID mapping (use replyJid so outbound replies go to the right place)
        this.senderTracker.registerSender(senderPhone, replyJid, isGroup);

        // ── Document message handling ──
        if (
          msg.message?.documentMessage ||
          msg.message?.documentWithCaptionMessage
        ) {
          if (this.config.mediaEnabled && this.ipcClient?.connected) {
            try {
              const docId = await this.mediaHandler.handleDocument(
                sock,
                msg,
                this.ipcClient,
              );
              if (docId) {
                // Document uploaded successfully, send docId as message text to daemon
                this.messageQueue.enqueue(
                  senderPhone,
                  `[文档已上传, id: ${docId}]`,
                  this.config.maxQueueSize,
                );
                if (!this.messageQueue.isProcessing(senderPhone)) {
                  this.processQueue(senderPhone, sock);
                }
              }
            } catch (err: any) {
              logger.error(`Document handling error: ${err.message}`);
            }
          }
          continue;
        }

        // ── Image message handling ──
        if (msg.message?.imageMessage) {
          if (this.config.mediaEnabled) {
            try {
              const imagePath = await this.mediaHandler.handleImage(sock, msg);
              if (imagePath) {
                // Image downloaded, send path as message content
                const caption =
                  msg.message.imageMessage.caption || "请描述这张图片";
                this.messageQueue.enqueue(
                  senderPhone,
                  `${caption}\n[图片路径: ${imagePath}]`,
                  this.config.maxQueueSize,
                );
                if (!this.messageQueue.isProcessing(senderPhone)) {
                  this.processQueue(senderPhone, sock);
                }
              }
            } catch (err: any) {
              logger.error(`Image handling error: ${err.message}`);
            }
          }
          continue;
        }

        // ── Text message ──
        const text =
          msg.message?.conversation ||
          msg.message?.extendedTextMessage?.text ||
          "";
        if (!text) continue;

        // Permission reply check (before command check)
        if (this.ipcClient?.connected) {
          const wasPermissionReply =
            await this.permissionManager.handleResponse(
              senderPhone,
              text,
              this.ipcClient,
            );
          if (wasPermissionReply) {
            try {
              await sock.sendMessage(replyJid, { text: "✅ 已处理。" });
            } catch {}
            continue;
          }
        }

        // Command check
        if (text.startsWith("/")) {
          const parsed = parseCommand(text);
          if (parsed && isRegisteredCommand(parsed.name)) {
            try {
              const result = await dispatchCommand({
                ipcClient: this.ipcClient!,
                args: text,
                sender: senderPhone,
                jid: replyJid,
                sock,
              });
              if (result) {
                const chunks = splitMessage(result);
                for (const chunk of chunks) {
                  try {
                    await sock.sendMessage(replyJid, { text: chunk });
                  } catch {}
                }
              }
            } catch (err: any) {
              try {
                await sock.sendMessage(replyJid, {
                  text: formatError("COMMAND_ERROR", err.message),
                });
              } catch {}
            }
            continue;
          } else if (parsed) {
            try {
              await sock.sendMessage(replyJid, {
                text: `❓ 未知命令 /${parsed.name}\n发送 /help 查看所有命令`,
              });
            } catch {}
            continue;
          }
        }

        // Normal message → queue
        const enqueued = this.messageQueue.enqueue(
          senderPhone,
          text,
          this.config.maxQueueSize,
        );
        if (!enqueued) {
          try {
            await sock.sendMessage(replyJid, {
              text: "⚠️ 消息队列已满，请稍后重试。",
            });
          } catch {}
          continue;
        }
        if (!this.messageQueue.isProcessing(senderPhone)) {
          this.processQueue(senderPhone, sock);
        }
      }
    });
  }

  /**
   * Process queued messages for a sender, one at a time.
   * Uses SenderTracker for JID lookup instead of passing jid directly.
   */
  private async processQueue(sender: string, sock: any): Promise<void> {
    this.messageQueue.startProcessing(sender);

    while (this.messageQueue.hasQueued(sender)) {
      const entry = this.messageQueue.dequeue(sender);
      if (!entry) break;

      // Clear accumulator for new response
      this.senderTracker.clearAccumulator(sender);
      this.activeSender = sender;
      this.processingFlags.add(sender);

      try {
        if (this.ipcClient?.connected) {
          await this.ipcClient.request("submitMessage", {
            prompt: entry.text,
            sender: sender,
          });
        }
      } catch (err: any) {
        logger.error(`submitMessage RPC error for ${sender}: ${err.message}`);
        const jid = this.senderTracker.getJid(sender);
        if (jid) {
          try {
            await sock.sendMessage(jid, {
              text: formatError("RPC_ERROR", err.message),
            });
          } catch {}
        }
        this.processingFlags.delete(sender);
        this.activeSender = null;
        continue;
      }

      // Wait for the result event to complete before processing next
      await this.waitForResult(sender);
      this.activeSender = null;
    }

    this.messageQueue.finishProcessing(sender);
  }

  /**
   * Wait for the daemon to emit a result/error event for the current interaction.
   * Uses processingFlags as the signal mechanism.
   */
  private waitForResult(sender: string): Promise<void> {
    return new Promise((resolve) => {
      const check = () => {
        if (!this.processingFlags.has(sender)) {
          resolve();
        } else {
          setTimeout(check, 100);
        }
      };
      check();
    });
  }

  /**
   * 4.3 — Outbound: daemon stream/event → accumulate assistant_chunk → send WhatsApp on result.
   * Integrates SenderTracker, PermissionManager, MediaHandler.
   */
  private setupStreamHandler(sock: any): void {
    if (!this.ipcClient) return;

    this.ipcClient.onNotification("stream/event", async (params: unknown) => {
      const event = params as Record<string, unknown>;
      if (!event || typeof event !== "object") return;

      const sender = this.activeSender;
      if (!sender) return;

      const jid = this.senderTracker.getJid(sender);

      switch (event.type) {
        case "assistant_chunk": {
          const content = (event as { content: string }).content || "";
          this.senderTracker.accumulate(sender, content);
          break;
        }

        case "tool_use": {
          const toolName =
            (event as { tool_name: string }).tool_name || "unknown";
          if (jid) {
            try {
              await sock.sendMessage(jid, { text: formatToolUse(toolName) });
            } catch {}
          }
          break;
        }

        case "tool_result": {
          const tr = event as { is_error: boolean; output: unknown };
          if (tr.is_error && jid) {
            const output =
              typeof tr.output === "string"
                ? tr.output
                : JSON.stringify(tr.output);
            try {
              await sock.sendMessage(jid, {
                text: formatError("TOOL_ERROR", output),
              });
            } catch {}
          }
          // Detect file paths in tool output and send them
          if (!tr.is_error && typeof tr.output === "string" && jid) {
            const filePaths = this.mediaHandler.detectFilePaths(tr.output);
            for (const fp of filePaths) {
              try {
                await this.mediaHandler.sendFile(sock, jid, fp);
              } catch (err) {
                logger.error(`Failed to send file ${fp}: ${err}`);
              }
            }
          }
          break;
        }

        case "permission_request": {
          const pr = event as {
            tool_use_id: string;
            tool_name: string;
            description?: string;
          };
          if (jid) {
            const text = this.permissionManager.formatPermissionRequest(
              pr.tool_use_id,
              pr.tool_name,
              pr.description,
            );
            try {
              await sock.sendMessage(jid, { text });
            } catch {}
            this.permissionManager.registerRequest(
              sender,
              pr.tool_use_id,
              pr.tool_name,
              pr.description || "",
              async (phone, toolUseId) => {
                // Timeout callback: notify user
                const j = this.senderTracker.getJid(phone);
                if (j) {
                  try {
                    await sock.sendMessage(j, {
                      text: "⏰ 权限请求已超时，自动拒绝。",
                    });
                  } catch {}
                }
              },
            );
          }
          break;
        }

        case "error": {
          const err = event as { code: string; message: string };
          if (jid) {
            try {
              await sock.sendMessage(jid, {
                text: formatError(
                  err.code || "ERROR",
                  err.message || "Unknown error",
                ),
              });
            } catch {}
          }
          this.senderTracker.clearAccumulator(sender);
          this.processingFlags.delete(sender);
          break;
        }

        case "result": {
          const accumulated = this.senderTracker.getAccumulated(sender);
          if (accumulated.length > 0 && jid) {
            const formatted = formatForWhatsApp(accumulated);
            const chunks = splitMessage(formatted);
            for (const chunk of chunks) {
              try {
                await sock.sendMessage(jid, { text: chunk });
              } catch {}
            }
          }
          // Check for file paths in the result
          if (jid) {
            const filePaths = this.mediaHandler.detectFilePaths(accumulated);
            for (const fp of filePaths) {
              try {
                await this.mediaHandler.sendFile(sock, jid, fp);
              } catch {}
            }
          }
          this.senderTracker.clearAccumulator(sender);
          this.processingFlags.delete(sender);
          break;
        }
      }
    });
  }

  /**
   * 4.4 — Graceful shutdown: SIGTERM/SIGINT, save auth, close UDS, 10s force exit.
   */
  private setupShutdownHandlers(sock: any): void {
    const shutdown = async (signal: string) => {
      if (this.shuttingDown) return;
      this.shuttingDown = true;
      logger.info(`\nShutdown initiated (${signal}).`);

      // Force exit after timeout
      const forceTimer = setTimeout(() => {
        logger.warn("Shutdown timeout exceeded (10s). Force exiting.");
        process.exit(1);
      }, SHUTDOWN_TIMEOUT_MS);
      forceTimer.unref();

      // Clean up PermissionManager timers
      this.permissionManager.cleanup();
      // Clean up MediaHandler temp files
      this.mediaHandler.cleanupAll();

      try {
        // Save auth state and disconnect WhatsApp
        await this.session.disconnect();
        logger.info("WhatsApp session saved and disconnected.");
      } catch (err) {
        logger.error(`Error disconnecting WhatsApp: ${err}`);
      }

      try {
        // Close UDS connection
        if (this.ipcClient) {
          await this.ipcClient.disconnect();
          logger.info("Daemon connection closed.");
        }
      } catch (err) {
        logger.error(`Error disconnecting daemon: ${err}`);
      }

      // Stop config watcher
      if (this.configWatcher) {
        this.configWatcher.close();
      }

      // Remove PID file
      this.removePidFile();

      clearTimeout(forceTimer);
      logger.info(`Shutdown complete (${signal}).`);
      process.exit(0);
    };

    process.on("SIGTERM", () => shutdown("SIGTERM"));
    process.on("SIGINT", () => shutdown("SIGINT"));
  }

  /**
   * Attempt to reconnect to a daemon after connection loss.
   * Uses exponential backoff up to reconnectMaxMs.
   */
  private async reconnectDaemon(sock: any, attempt: number = 1): Promise<void> {
    const RECONNECT_BASE_MS = 5_000;
    const maxMs = this.config.reconnectMaxMs || 300_000;
    const delay = Math.min(RECONNECT_BASE_MS * Math.pow(2, attempt - 1), maxMs);

    logger.warn(`Reconnect attempt ${attempt} in ${delay}ms...`);
    await new Promise((r) => setTimeout(r, delay));

    try {
      const { client, info } = await this.daemonConnector.discoverAndConnect(
        60_000,
        5_000,
        this.config.sharedSessionId,
      );
      this.ipcClient = client;
      this.daemonInfo = info;
      setDaemonMetrics({
        reconnectCount: this.daemonConnector.reconnectCount,
        lastConnectAt: this.daemonConnector.lastConnectAt,
      });
      this.setupStreamHandler(sock);
      client.onDisconnect(() => {
        if (!this.shuttingDown) {
          logger.warn("Daemon connection lost. Reconnecting...");
          this.reconnectDaemon(sock, 1); // reset attempt
        }
      });
      logger.info(`Reconnected to daemon pid=${info.pid}`);
    } catch (err) {
      logger.error(`Reconnect attempt ${attempt} failed: ${err}`);
      this.reconnectDaemon(sock, attempt + 1);
    }
  }

  /**
   * 4.5 — Write PID file ~/.baoclaw/whatsapp-gateway.pid
   */
  private writePidFile(): void {
    const pidData = {
      pid: process.pid,
      phone: this.session.getPhoneNumber(),
      daemon_session_id: this.daemonInfo?.session_id ?? null,
      started_at: new Date().toISOString(),
    };
    try {
      const dir = path.dirname(PID_FILE);
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(PID_FILE, JSON.stringify(pidData, null, 2));
      logger.info(`PID file written: ${PID_FILE}`);
    } catch (err) {
      logger.warn(`Failed to write PID file: ${err}`);
    }
  }

  private removePidFile(): void {
    try {
      fs.unlinkSync(PID_FILE);
    } catch {
      /* ignore */
    }
  }

  /**
   * Stop the gateway with a reason.
   */
  async stop(reason: string): Promise<void> {
    logger.info(`Stopping gateway: ${reason}`);
    process.emit("SIGTERM" as any);
  }
}

// ── Entry point ──
async function main() {
  const gateway = new WhatsAppGateway();
  try {
    await gateway.start();
  } catch (err) {
    logger.error(`Gateway failed to start: ${err}`);
    process.exit(1);
  }
}

main();
