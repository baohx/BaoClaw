/**
 * WhatsApp Gateway — main process.
 * Orchestrates: load config → Baileys init → daemon discover → message loop.
 * Handles inbound WhatsApp messages, outbound daemon responses,
 * graceful shutdown, and PID file management.
 */
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { loadWhatsAppConfig, watchConfig, type WhatsAppConfig } from './config.js';
import { isAllowed, validateE164, normalizeJid } from './allowlist.js';
import { RateLimiter } from './rateLimiter.js';
import { formatForWhatsApp, splitMessage, formatToolUse, formatError } from './formatter.js';
import { MessageQueue } from './messageQueue.js';
import { DaemonConnector, type DaemonInfo } from './daemon.js';
import { SessionManager } from './session.js';
import { IpcClient } from './ipcClient.js';

const PID_FILE = path.join(os.homedir(), '.baoclaw', 'whatsapp-gateway.pid');
const SHUTDOWN_TIMEOUT_MS = 10_000;

export interface GatewayOptions {
  configPath?: string;
}

export class WhatsAppGateway {
  private config!: WhatsAppConfig;
  private configWatcher: fs.FSWatcher | null = null;
  private session: SessionManager;
  private daemonConnector: DaemonConnector;
  private ipcClient: IpcClient | null = null;
  private daemonInfo: DaemonInfo | null = null;
  private rateLimiter: RateLimiter;
  private messageQueue: MessageQueue;
  private shuttingDown = false;
  private configPath: string;

  // Per-sender response accumulator: sender → accumulated text
  private responseAccumulators = new Map<string, string>();
  // Track which sender triggered the current daemon interaction
  private activeSender: string | null = null;

  constructor(options?: GatewayOptions) {
    this.configPath = options?.configPath ?? path.join(os.homedir(), '.baoclaw', 'config.json');
    this.session = new SessionManager(undefined, undefined); // phone set after config load
    this.daemonConnector = new DaemonConnector();
    this.rateLimiter = new RateLimiter();
    this.messageQueue = new MessageQueue();
  }

  /**
   * 4.1 — Start the gateway: load config → Baileys init → daemon discover → message loop.
   */
  async start(): Promise<void> {
    console.log('WhatsApp Gateway starting...');

    // Load config
    this.config = loadWhatsAppConfig(this.configPath);

    if (!this.config.enabled) {
      console.log('WhatsApp is disabled in configuration (whatsapp.enabled = false). Exiting.');
      process.exit(0);
    }

    // Validate allowlist entries
    const validAllow: string[] = [];
    for (const entry of this.config.allowFrom) {
      if (validateE164(entry)) {
        validAllow.push(entry);
      } else {
        console.warn(`Invalid E.164 number in allowFrom, skipping: ${entry}`);
      }
    }
    this.config.allowFrom = validAllow;

    if (validAllow.length === 0) {
      console.warn('Warning: allowFrom is empty — all incoming messages will be rejected.');
    }

    // Watch config for hot-reload
    this.configWatcher = watchConfig(this.configPath, (newConfig) => {
      console.log('Config reloaded.');
      this.config = newConfig;
    });

    // Initialize Baileys session with phone number from config
    this.session = new SessionManager(undefined, this.config.phoneNumber ?? undefined);
    console.log('Initializing WhatsApp connection...');
    const sock = await this.session.initialize();

    // Discover and connect to daemon
    console.log('Discovering BaoClaw daemon...');
    const { client, info } = await this.daemonConnector.discoverAndConnect();
    this.ipcClient = client;
    this.daemonInfo = info;
    console.log(`Connected to daemon pid=${info.pid} session=${info.session_id}`);

    // Set up daemon stream event handler (4.3 — outbound)
    this.setupStreamHandler(sock);

    // Set up daemon disconnect handler
    client.onDisconnect(() => {
      if (!this.shuttingDown) {
        console.warn('Daemon connection lost. Attempting reconnect...');
        this.reconnectDaemon(sock);
      }
    });

    // Set up inbound message handler (4.2)
    this.setupInboundHandler(sock);

    // Write PID file (4.5)
    this.writePidFile();

    // Set up graceful shutdown (4.4)
    this.setupShutdownHandlers(sock);

    console.log('WhatsApp Gateway is ready.');
  }

  /**
   * 4.2 — Inbound: WhatsApp msg → allowlist → rate limit → queue → submitMessage RPC.
   */
  private setupInboundHandler(sock: any): void {
    sock.ev.on('messages.upsert', async (m: any) => {
      if (this.shuttingDown) return;
      const messages = m.messages || [];

      for (const msg of messages) {
        // Skip non-text, status broadcasts, and own messages
        if (!msg.message?.conversation && !msg.message?.extendedTextMessage?.text) continue;
        if (msg.key.fromMe) continue;
        if (msg.key.remoteJid === 'status@broadcast') continue;

        const jid = msg.key.remoteJid!;
        const isGroup = jid.endsWith('@g.us');
        const text = msg.message?.conversation || msg.message?.extendedTextMessage?.text || '';

        // Policy check
        if (isGroup && this.config.groupPolicy === 'ignore') continue;
        if (!isGroup && this.config.dmPolicy === 'ignore') continue;

        // Determine sender phone
        const senderJid = isGroup ? (msg.key.participant || jid) : jid;
        const senderPhone = normalizeJid(senderJid);

        // Allowlist check
        if (!isAllowed(senderPhone, this.config.allowFrom)) {
          console.log(`Rejected message from non-allowlisted sender: ${senderPhone}`);
          continue;
        }

        // Rate limit check
        if (!this.rateLimiter.tryConsume(senderPhone)) {
          console.log(`Rate limited sender: ${senderPhone}`);
          try {
            await sock.sendMessage(jid, {
              text: '⏳ Rate limit exceeded. Please wait before sending more messages.',
            });
          } catch { /* ignore send errors */ }
          continue;
        }

        // Enqueue and process
        this.messageQueue.enqueue(senderPhone, text);
        if (!this.messageQueue.isProcessing(senderPhone)) {
          this.processQueue(senderPhone, jid, sock);
        }
      }
    });
  }

  /**
   * Process queued messages for a sender, one at a time.
   */
  private async processQueue(sender: string, jid: string, sock: any): Promise<void> {
    this.messageQueue.startProcessing(sender);

    while (this.messageQueue.hasQueued(sender)) {
      const entry = this.messageQueue.dequeue(sender);
      if (!entry) break;

      this.activeSender = sender;
      this.responseAccumulators.set(sender, '');

      try {
        if (this.ipcClient?.connected) {
          await this.ipcClient.request('submitMessage', {
            prompt: entry.text,
            sender: sender,
          });
        }
      } catch (err: any) {
        console.error(`submitMessage RPC error for ${sender}: ${err.message}`);
        try {
          await sock.sendMessage(jid, {
            text: formatError('RPC_ERROR', err.message),
          });
        } catch { /* ignore */ }
      }

      // Wait for the result event to complete before processing next
      await this.waitForResult(sender);
      this.activeSender = null;
    }

    this.messageQueue.finishProcessing(sender);
  }

  /**
   * Wait for the daemon to emit a result event for the current interaction.
   */
  private waitForResult(sender: string): Promise<void> {
    return new Promise((resolve) => {
      const check = () => {
        // Result handler will delete the accumulator when done
        if (!this.responseAccumulators.has(sender)) {
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
   */
  private setupStreamHandler(sock: any): void {
    if (!this.ipcClient) return;

    this.ipcClient.onNotification('stream/event', async (params: unknown) => {
      const event = params as Record<string, unknown>;
      if (!event || typeof event !== 'object') return;

      const sender = this.activeSender;
      if (!sender) return;

      // Find the JID to reply to — we need to look it up from the sender phone
      // For simplicity, we store the JID mapping when processing inbound
      const jid = sender.replace('+', '') + '@s.whatsapp.net';

      switch (event.type) {
        case 'assistant_chunk': {
          const content = (event as { content: string }).content || '';
          const current = this.responseAccumulators.get(sender) ?? '';
          this.responseAccumulators.set(sender, current + content);
          break;
        }

        case 'tool_use': {
          const toolName = (event as { tool_name: string }).tool_name || 'unknown';
          try {
            await sock.sendMessage(jid, { text: formatToolUse(toolName) });
          } catch { /* ignore */ }
          break;
        }

        case 'tool_result': {
          const tr = event as { is_error: boolean; output: unknown };
          if (tr.is_error) {
            const output = typeof tr.output === 'string' ? tr.output : JSON.stringify(tr.output);
            try {
              await sock.sendMessage(jid, {
                text: formatError('TOOL_ERROR', output),
              });
            } catch { /* ignore */ }
          }
          break;
        }

        case 'error': {
          const err = event as { code: string; message: string };
          try {
            await sock.sendMessage(jid, {
              text: formatError(err.code || 'ERROR', err.message || 'Unknown error'),
            });
          } catch { /* ignore */ }
          // Clean up accumulator so processQueue can continue
          this.responseAccumulators.delete(sender);
          break;
        }

        case 'result': {
          // Send accumulated response
          const accumulated = this.responseAccumulators.get(sender) ?? '';
          if (accumulated.length > 0) {
            const formatted = formatForWhatsApp(accumulated);
            const chunks = splitMessage(formatted);
            for (const chunk of chunks) {
              try {
                await sock.sendMessage(jid, { text: chunk });
              } catch (err) {
                console.error(`Failed to send WhatsApp message: ${err}`);
              }
            }
          }
          // Clean up accumulator — signals processQueue to continue
          this.responseAccumulators.delete(sender);
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
      console.log(`\nShutdown initiated (${signal}).`);

      // Force exit after timeout
      const forceTimer = setTimeout(() => {
        console.warn('Shutdown timeout exceeded (10s). Force exiting.');
        process.exit(1);
      }, SHUTDOWN_TIMEOUT_MS);
      forceTimer.unref();

      try {
        // Save auth state and disconnect WhatsApp
        await this.session.disconnect();
        console.log('WhatsApp session saved and disconnected.');
      } catch (err) {
        console.error(`Error disconnecting WhatsApp: ${err}`);
      }

      try {
        // Close UDS connection
        if (this.ipcClient) {
          await this.ipcClient.disconnect();
          console.log('Daemon connection closed.');
        }
      } catch (err) {
        console.error(`Error disconnecting daemon: ${err}`);
      }

      // Stop config watcher
      if (this.configWatcher) {
        this.configWatcher.close();
      }

      // Remove PID file
      this.removePidFile();

      clearTimeout(forceTimer);
      console.log(`Shutdown complete (${signal}).`);
      process.exit(0);
    };

    process.on('SIGTERM', () => shutdown('SIGTERM'));
    process.on('SIGINT', () => shutdown('SIGINT'));
  }

  /**
   * Attempt to reconnect to a daemon after connection loss.
   */
  private async reconnectDaemon(sock: any): Promise<void> {
    try {
      const { client, info } = await this.daemonConnector.discoverAndConnect();
      this.ipcClient = client;
      this.daemonInfo = info;
      this.setupStreamHandler(sock);
      client.onDisconnect(() => {
        if (!this.shuttingDown) {
          console.warn('Daemon connection lost again. Attempting reconnect...');
          this.reconnectDaemon(sock);
        }
      });
      console.log(`Reconnected to daemon pid=${info.pid}`);
    } catch (err) {
      console.error(`Failed to reconnect to daemon: ${err}`);
      console.error('No daemon available. Shutting down.');
      process.exit(1);
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
      console.log(`PID file written: ${PID_FILE}`);
    } catch (err) {
      console.warn(`Failed to write PID file: ${err}`);
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
    console.log(`Stopping gateway: ${reason}`);
    process.emit('SIGTERM' as any);
  }
}

// ── Entry point ──
async function main() {
  const gateway = new WhatsAppGateway();
  try {
    await gateway.start();
  } catch (err) {
    console.error(`Gateway failed to start: ${err}`);
    process.exit(1);
  }
}

main();
