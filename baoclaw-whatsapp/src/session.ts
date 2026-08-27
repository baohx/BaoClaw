/**
 * Baileys Session Manager (v7.x).
 * WhatsApp Web connection via QR code (default) or pairing code (--pairing flag).
 */
import * as fs from "fs";
import * as path from "path";
import * as os from "os";
import * as readline from "readline";
import { createLogger } from "../../ts-ipc/logger.js";

const runtimeLogger = createLogger("whatsapp");
const log = (level: "info" | "warn" | "error", args: unknown[]) =>
  runtimeLogger[level](args.map(String).join(" "));

let makeWASocket: any;
let useMultiFileAuthState: any;
let DisconnectReason: any;
let Browsers: any;
let fetchLatestBaileysVersion: any;

async function loadDeps() {
  const baileys = await import("@whiskeysockets/baileys");
  makeWASocket = baileys.makeWASocket ?? baileys.default;
  useMultiFileAuthState = baileys.useMultiFileAuthState;
  DisconnectReason = baileys.DisconnectReason;
  Browsers = baileys.Browsers;
  fetchLatestBaileysVersion = baileys.fetchLatestBaileysVersion;
}

const AUTH_DIR_NAME = "whatsapp-auth";
const MAX_RETRIES = 5;
const usePairingMode = process.argv.includes("--pairing");

const logger = {
  level: "warn" as const,
  info: (...args: any[]) => log("info", args),
  warn: (...args: any[]) => log("warn", ["[Baileys warn]", ...args]),
  error: (...args: any[]) => log("error", ["[Baileys error]", ...args]),
  debug: () => {},
  trace: () => {},
  fatal: (...args: any[]) => log("error", ["[Baileys fatal]", ...args]),
  child: () => logger,
} as any;

export function getAuthDir(): string {
  return path.join(os.homedir(), ".baoclaw", AUTH_DIR_NAME);
}

function secureAuthDirectory(authDir: string): void {
  fs.chmodSync(authDir, 0o700);
  for (const entry of fs.readdirSync(authDir, { withFileTypes: true })) {
    if (entry.isFile()) fs.chmodSync(path.join(authDir, entry.name), 0o600);
  }
}

function prompt(question: string): Promise<string> {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
  });
  return new Promise((resolve) => {
    rl.question(question, (answer) => {
      rl.close();
      resolve(answer.trim());
    });
  });
}

function displayQR(qr: string) {
  // qrcode-terminal prints directly to stdout when called without callback
  try {
    // ESM dynamic import of CJS — qrcode-terminal writes to process.stdout
    import("qrcode-terminal")
      .then((mod: any) => {
        const qt = mod.default ?? mod;
        if (typeof qt.generate === "function") {
          qt.generate(qr, { small: true });
        } else {
          printQRAsURL(qr);
        }
      })
      .catch(() => printQRAsURL(qr));
  } catch {
    printQRAsURL(qr);
  }
}

function printQRAsURL(qr: string) {
  runtimeLogger.info(
    `\n📷 Open this URL in browser to get QR code, then scan with WhatsApp:\n`,
  );
  runtimeLogger.info(
    `https://api.qrserver.com/v1/create-qr-code/?size=400x400&data=${encodeURIComponent(qr)}\n`,
  );
}

export class SessionManager {
  private sock: any = null;
  private phoneNumber: string | null = null;
  private _isConnected = false;
  private authDir: string;
  private pairingPhone: string | null;
  private proxyUrl: string | null = null;
  private proxyAgent: any = undefined;

  constructor(authDir?: string, pairingPhone?: string, proxyUrl?: string) {
    this.authDir = authDir ?? getAuthDir();
    this.pairingPhone = pairingPhone ?? null;
    this.proxyUrl = proxyUrl ?? null;
  }

  private async initProxy(): Promise<void> {
    if (this.proxyUrl && !this.proxyAgent) {
      try {
        const mod = await import("socks-proxy-agent");
        const SocksProxyAgent = mod.SocksProxyAgent || (mod as any).default;
        this.proxyAgent = new SocksProxyAgent(this.proxyUrl);
        runtimeLogger.info(`Using proxy: ${this.proxyUrl}`);
      } catch (err: any) {
        runtimeLogger.warn(`Failed to create proxy agent: ${err.message}`);
      }
    }
  }

  async initialize(): Promise<any> {
    await loadDeps();
    await this.initProxy();

    let waVersion: number[] | undefined;
    try {
      const latest = await fetchLatestBaileysVersion();
      if (latest?.version) waVersion = latest.version;
    } catch (err: any) {
      runtimeLogger.warn(
        `Could not fetch latest WhatsApp version: ${err.message}`,
      );
    }

    fs.mkdirSync(this.authDir, { recursive: true, mode: 0o700 });
    secureAuthDirectory(this.authDir);
    const { state, saveCreds } = await useMultiFileAuthState(this.authDir);
    const hasAuth =
      fs.existsSync(path.join(this.authDir, "creds.json")) &&
      state.creds?.registered;

    return new Promise((resolve, reject) => {
      let retries = 0;
      let resolved = false;
      let pairingRequested = false;

      const startSocket = () => {
        const browserConfig = Browsers
          ? Browsers.ubuntu("Chrome")
          : ["BaoClaw", "Chrome", "22.04"];

        const sock = makeWASocket({
          auth: state,
          browser: browserConfig,
          connectTimeoutMs: 60_000,
          logger,
          ...(waVersion ? { version: waVersion } : {}),
          ...(this.proxyAgent
            ? { agent: this.proxyAgent, fetchAgent: this.proxyAgent }
            : {}),
        });

        sock.ev.on("creds.update", () => {
          void saveCreds()
            .then(() => secureAuthDirectory(this.authDir))
            .catch((err: unknown) => {
              const message = err instanceof Error ? err.message : String(err);
              logger.error(`Failed to save auth credentials: ${message}`);
            });
        });

        sock.ev.on("connection.update", async (update: any) => {
          const { connection, lastDisconnect, qr } = update;

          if (qr) {
            if (usePairingMode && !pairingRequested) {
              pairingRequested = true;
              try {
                let phone = this.pairingPhone;
                if (!phone) {
                  phone = await prompt(
                    "\n📱 Enter WhatsApp phone (e.g. +8613812345678): ",
                  );
                }
                const cleaned = phone.replace(/[^0-9]/g, "");
                runtimeLogger.info(
                  `\nRequesting pairing code for +${cleaned}...`,
                );
                const code = await sock.requestPairingCode(cleaned);
                runtimeLogger.info(`\n🔑 Pairing code: ${code}`);
                runtimeLogger.info(
                  `Open WhatsApp → Settings → Linked Devices → Link a Device`,
                );
                runtimeLogger.info(
                  `Choose "Link with phone number instead" and enter the code.\n`,
                );
              } catch (err: any) {
                runtimeLogger.error(`Pairing code failed: ${err.message}`);
                runtimeLogger.info("\nFalling back to QR code:");
                await displayQR(qr);
              }
            } else {
              runtimeLogger.info("\n📱 Scan this QR code with WhatsApp:");
              await displayQR(qr);
              runtimeLogger.info(
                "Open WhatsApp → Settings → Linked Devices → Link a Device → Scan QR\n",
              );
            }
          }

          if (connection === "open" && !resolved) {
            resolved = true;
            this.sock = sock;
            this._isConnected = true;
            this.phoneNumber = sock.user?.id
              ? "+" + sock.user.id.split(":")[0]
              : null;
            runtimeLogger.info(
              `\n✅ WhatsApp connected${this.phoneNumber ? ` as ${this.phoneNumber}` : ""}.`,
            );
            resolve(sock);
          }

          if (connection === "close" && !resolved) {
            this._isConnected = false;
            const statusCode = (lastDisconnect?.error as any)?.output
              ?.statusCode;
            const isLoggedOut = statusCode === DisconnectReason?.loggedOut;
            if (isLoggedOut) {
              runtimeLogger.info("Logged out. Clearing auth state.");
              this.clearAuthState();
              reject(new Error("Logged out from WhatsApp"));
              return;
            }
            retries++;
            if (retries > MAX_RETRIES) {
              reject(
                new Error(
                  `Failed after ${MAX_RETRIES} retries (status=${statusCode})`,
                ),
              );
              return;
            }
            runtimeLogger.info(
              `Connection closed (status=${statusCode}). Retry ${retries}/${MAX_RETRIES} in 3s...`,
            );
            setTimeout(() => {
              if (!resolved) startSocket();
            }, 3000);
          }
        });
      };

      if (!hasAuth) {
        runtimeLogger.info(
          `\n📱 Mode: ${usePairingMode ? "Pairing Code" : "QR Code scan"}`,
        );
      }
      startSocket();
    });
  }

  getPhoneNumber(): string | null {
    return this.phoneNumber;
  }
  isConnected(): boolean {
    return this._isConnected;
  }
  getSocket(): any {
    return this.sock;
  }

  async disconnect(): Promise<void> {
    if (this.sock) {
      try {
        this.sock.end(undefined);
      } catch {}
      this.sock = null;
      this._isConnected = false;
    }
  }

  clearAuthState(): void {
    try {
      fs.rmSync(this.authDir, { recursive: true, force: true });
    } catch {}
  }
}
