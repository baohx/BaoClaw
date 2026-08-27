import * as fs from "node:fs";

/** Restrict a local secret-bearing file to its owner when supported. */
export function securePrivateFile(filePath: string): void {
  try {
    if (fs.existsSync(filePath)) fs.chmodSync(filePath, 0o600);
  } catch {
    // Permission hardening is best effort on platforms without POSIX modes.
  }
}
