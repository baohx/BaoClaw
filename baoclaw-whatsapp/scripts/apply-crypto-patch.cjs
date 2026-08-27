const { createCipheriv, createDecipheriv } = require("node:crypto");
const { spawnSync } = require("node:child_process");

function nativeAesWorks() {
  try {
    const key = Buffer.alloc(32, 1);
    const iv = Buffer.alloc(16, 2);
    const cipher = createCipheriv("aes-256-cbc", key, iv);
    const encrypted = Buffer.concat([cipher.update("baoclaw"), cipher.final()]);
    const decipher = createDecipheriv("aes-256-cbc", key, iv);
    const decrypted = Buffer.concat([
      decipher.update(encrypted),
      decipher.final(),
    ]);
    return decrypted.toString() === "baoclaw";
  } catch {
    return false;
  }
}

if (nativeAesWorks()) {
  console.log(
    "[crypto-patch] Native AES is working; skipping Baileys/libsignal patches.",
  );
  process.exit(0);
}

console.warn(
  "[crypto-patch] Native AES is failing; applying Baileys/libsignal patches.",
);
const patchPackage = require.resolve("patch-package");
const result = spawnSync(process.execPath, [patchPackage], {
  stdio: "inherit",
});
process.exit(result.status ?? 1);
