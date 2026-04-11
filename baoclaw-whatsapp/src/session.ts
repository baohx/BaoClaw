/**
 * Baileys Session Manager.
 * Handles WhatsApp Web initialization via pairing code,
 * auth state persistence, and reconnection.
 */
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import * as readline from 'readline';

let makeWASocket: any;
let useMultiFileAuthState: any;
let DisconnectReason: any;

async function loadDeps() {
  const baileys = await import('@whiskeysockets/baileys');
  makeWASocket = baileys.default || baileys.makeWASocket;
  useMultiFileAuthState = baileys.useMultiFileAuthState;
  DisconnectReason = baileys.DisconnectReason;
}

const AUTH_DIR_NAME = 'whatsapp-auth';
const MAX_RETRIES = 5;

// Silent logger to suppress Baileys JSON noise
const silentLogger = {
  level: 'silent',
  info: () => {}, warn: () => {}, error: (...args: any[]) => console.error(...args),
  debug: () => {}, trace: () => {}, fatal: (...args: any[]) => console.error(...args),
  child: () => silentLogger,
} as any;

export function getAuthDir(): string {
  return path.join(os.homedir(), '.baoclaw', AUTH_DIR_NAME);
}

/** Prompt user for input on stdin */
function prompt(question: string): Promise<string> {
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  return new Promise((resolve) => {
    rl.question(question, (answer) => {
      rl.close();
      resolve(answer.trim());
    });
  });
}

export class SessionManager {
  private sock: any = null;
  private phoneNumber: string | null = null;
  private _isConnected = false;
  private authDir: string;
  private pairingPhone: string | null;

  constructor(authDir?: string, pairingPhone?: string) {
    this.authDir = authDir ?? getAuthDir();
    this.pairingPhone = pairingPhone ?? null;
  }

  /**
   * Initialize the Baileys session using pairing code mode.
   * - If auth state exists, restores session automatically.
   * - If no auth state, asks for phone number and requests a pairing code.
   */
  async initialize(): Promise<any> {
    await loadDeps();

    fs.mkdirSync(this.authDir, { recursive: true, mode: 0o700 });
    const { state, saveCreds } = await useMultiFileAuthState(this.authDir);
    const hasAuth = fs.existsSync(path.join(this.authDir, 'creds.json'))
      && state.creds?.registered;

    return new Promise((resolve, reject) => {
      let retries = 0;
      let resolved = false;
      let pairingRequested = false;

      const startSocket = () => {
        const sock = makeWASocket({
          auth: state,
          browser: ['BaoClaw', 'Chrome', '22.04'],
          connectTimeoutMs: 60_000,
          logger: silentLogger,
        });

        sock.ev.on('creds.update', saveCreds);

        sock.ev.on('connection.update', async (update: any) => {
          const { connection, lastDisconnect } = update;

          if (connection === 'open' && !resolved) {
            resolved = true;
            this.sock = sock;
            this._isConnected = true;
            this.phoneNumber = sock.user?.id
              ? '+' + sock.user.id.split(':')[0]
              : null;
            console.log(
              `\n✅ WhatsApp connected${this.phoneNumber ? ` as ${this.phoneNumber}` : ''}.`,
            );
            resolve(sock);
          }

          if (connection === 'close' && !resolved) {
            this._isConnected = false;
            const statusCode =
              (lastDisconnect?.error as any)?.output?.statusCode;
            const isLoggedOut = statusCode === DisconnectReason?.loggedOut;

            if (isLoggedOut) {
              console.log('Logged out. Clearing auth state.');
              this.clearAuthState();
              reject(new Error('Logged out from WhatsApp'));
              return;
            }

            retries++;
            if (retries > MAX_RETRIES) {
              reject(new Error(`Failed to connect after ${MAX_RETRIES} retries (last status: ${statusCode})`));
              return;
            }

            console.log(`Connection closed (status=${statusCode}). Retrying ${retries}/${MAX_RETRIES} in 3s...`);
            setTimeout(() => {
              if (!resolved) startSocket();
            }, 3000);
          }
        });

        // Request pairing code if not already authenticated
        if (!hasAuth && !pairingRequested) {
          pairingRequested = true;
          // Wait a moment for the socket to connect before requesting pairing
          setTimeout(async () => {
            try {
              let phone = this.pairingPhone;
              if (!phone) {
                phone = await prompt('\n📱 Enter your WhatsApp phone number (with country code, e.g. +8613812345678): ');
              }
              // Strip non-digits except leading +
              const cleaned = phone.replace(/[^0-9]/g, '');
              if (cleaned.length < 7) {
                console.error('Invalid phone number. Must include country code.');
                reject(new Error('Invalid phone number'));
                return;
              }
              console.log(`\nRequesting pairing code for +${cleaned}...`);
              const code = await sock.requestPairingCode(cleaned);
              console.log(`\n🔑 Pairing code: ${code}`);
              console.log(`\nOpen WhatsApp on your phone → Settings → Linked Devices → Link a Device`);
              console.log(`Choose "Link with phone number instead" and enter the code above.\n`);
            } catch (err: any) {
              console.error(`Failed to request pairing code: ${err.message}`);
              // Don't reject — the reconnect loop will retry
            }
          }, 5000);
        }
      };

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
      try { this.sock.end(undefined); } catch { /* ignore */ }
      this.sock = null;
      this._isConnected = false;
    }
  }

  clearAuthState(): void {
    try { fs.rmSync(this.authDir, { recursive: true, force: true }); } catch { /* ignore */ }
  }
}
