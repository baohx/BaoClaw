/**
 * Minimal Baileys receive test.
 * Connects to WhatsApp, prints ALL events, and logs any incoming messages.
 * Run: npx tsx test-receive.ts
 */
import { createRequire } from "module";
const require = createRequire(import.meta.url);

// Load the patched session module (which loads Baileys with our AES shim)
const baileys = await import("@whiskeysockets/baileys");
const { makeWASocket, useMultiFileAuthState, DisconnectReason, Browsers } =
  baileys;

import * as path from "path";
import * as os from "os";
import * as fs from "fs";

const AUTH_DIR = path.join(os.homedir(), ".baoclaw", "whatsapp-auth");
fs.mkdirSync(AUTH_DIR, { recursive: true });

const { state, saveCreds } = await useMultiFileAuthState(AUTH_DIR);

console.log("Starting minimal Baileys test...");
console.log("Auth dir:", AUTH_DIR);
console.log("Has creds:", fs.existsSync(path.join(AUTH_DIR, "creds.json")));

const sock = makeWASocket({
  auth: state,
  browser: Browsers.ubuntu("Chrome"),
  connectTimeoutMs: 60_000,
  printQRInTerminal: true,
});

sock.ev.on("creds.update", saveCreds);

sock.ev.on("connection.update", (update: any) => {
  console.log("[connection.update]", JSON.stringify(update, null, 2));
  if (update.connection === "open") {
    console.log("\n✅ Connected! Waiting for messages...");
    console.log(
      "Send a message from another phone to this WhatsApp account.\n",
    );
  }
  if (update.connection === "close") {
    const code = (update.lastDisconnect?.error as any)?.output?.statusCode;
    console.log(`\n❌ Connection closed (code=${code})`);
    if (code === DisconnectReason.loggedOut) {
      console.log("Logged out. Delete auth dir and re-pair.");
    }
    process.exit(1);
  }
});

sock.ev.on("messages.upsert", (m: any) => {
  console.log(
    "\n📨 [messages.upsert] type:",
    m.type,
    "count:",
    m.messages?.length,
  );
  for (const msg of m.messages || []) {
    console.log("  key:", JSON.stringify(msg.key));
    console.log("  fromMe:", msg.key.fromMe);
    const text =
      msg.message?.conversation ??
      msg.message?.extendedTextMessage?.text ??
      "(non-text)";
    console.log("  text:", text);
    console.log("  full message keys:", Object.keys(msg.message || {}));
    console.log("");
  }
});

// Also listen for message-receipt-update (read receipts etc)
sock.ev.on("message-receipt.update" as any, (updates: any) => {
  console.log("[message-receipt.update]", updates?.length, "updates");
});

// Catch any errors
sock.ev.on("messaging-history.set" as any, (data: any) => {
  console.log(
    "[messaging-history.set] messages:",
    data?.messages?.length,
    "isLatest:",
    data?.isLatest,
  );
});

console.log("\nListening for all events. Press Ctrl+C to stop.\n");
