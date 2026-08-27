/**
 * Integration test: verify that Baileys' crypto utilities, after patching,
 * round-trip AES-256-CBC correctly on this hardware.
 *
 * Tests both layers:
 *   1. Baileys' aesEncrypt/aesDecrypt (uses createCipheriv from the shim).
 *   2. libsignal's crypto.encrypt/decrypt (uses aesShim from the shim).
 */

const RED = "\x1b[31m";
const GREEN = "\x1b[32m";
const RESET = "\x1b[0m";

let failed = 0;

function assert(name: string, ok: boolean, detail = ""): void {
  if (ok) {
    console.log(`  ${GREEN}✓${RESET} ${name}`);
  } else {
    console.log(`  ${RED}✗${RESET} ${name} ${detail}`);
    failed++;
  }
}

console.log("cryptoPatch integration test\n");

// Test 1: Baileys' aesEncrypt/aesDecrypt
try {
  const baileysCrypto: any =
    await import("@whiskeysockets/baileys/lib/Utils/crypto.js");
  if (baileysCrypto.aesEncrypt && baileysCrypto.aesDecrypt) {
    const key = Buffer.alloc(32, 0x33);
    const plain = Buffer.from("Baileys round-trip test");
    const enc = baileysCrypto.aesEncrypt(plain, key);
    const dec = baileysCrypto.aesDecrypt(enc, key);
    assert("Baileys aesEncrypt/aesDecrypt (CBC) round-trip", plain.equals(dec));
  } else {
    assert(
      "Baileys crypto util loadable",
      false,
      "(aesEncrypt/aesDecrypt not exported)",
    );
  }
} catch (err: any) {
  assert("Baileys crypto util loadable", false, `(${err.message})`);
}

// Test 2: Baileys' aesEncrypWithIV (typo in upstream) and aesDecryptWithIV
try {
  const baileysCrypto: any =
    await import("@whiskeysockets/baileys/lib/Utils/crypto.js");
  if (baileysCrypto.aesEncrypWithIV && baileysCrypto.aesDecryptWithIV) {
    const key = Buffer.alloc(32, 0x44);
    const iv = Buffer.alloc(16, 0x55);
    const plain = Buffer.from("explicit IV variant");
    const enc = baileysCrypto.aesEncrypWithIV(plain, key, iv);
    const dec = baileysCrypto.aesDecryptWithIV(enc, key, iv);
    assert(
      "Baileys aesEncrypWithIV/aesDecryptWithIV round-trip",
      plain.equals(dec),
    );
  } else {
    assert("Baileys aesEncrypWithIV present", false);
  }
} catch (err: any) {
  assert("Baileys aesEncrypWithIV", false, `(${err.message})`);
}

// Test 3: libsignal's crypto.encrypt/decrypt
try {
  const { createRequire } = await import("module");
  const requireCjs = createRequire(import.meta.url);
  const libsignalCrypto: any = requireCjs("libsignal/src/crypto.js");
  const key = Buffer.alloc(32, 0x66);
  const iv = Buffer.alloc(16, 0x77);
  const plain = Buffer.from("libsignal round-trip test");
  const padLen = 16 - (plain.length % 16);
  const padded = Buffer.concat([plain, Buffer.alloc(padLen, padLen)]);
  const enc = libsignalCrypto.encrypt(key, padded, iv);
  const dec = libsignalCrypto.decrypt(key, enc, iv);
  assert("libsignal encrypt/decrypt round-trip", padded.equals(dec));
} catch (err: any) {
  assert("libsignal crypto loadable", false, `(${err.message})`);
}

console.log("");
if (failed === 0) {
  console.log(`${GREEN}All integration tests passed.${RESET}`);
  process.exit(0);
} else {
  console.log(`${RED}${failed} integration test(s) failed.${RESET}`);
  process.exit(1);
}
