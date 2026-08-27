import * as fs from "fs";

const LEVELS = { DEBUG: 0, INFO: 1, WARN: 2, ERROR: 3 } as const;
type Level = keyof typeof LEVELS;

let currentLevel: number = LEVELS.INFO;
let logFilePath: string | null = null;
const MAX_LOG_BYTES = 5 * 1024 * 1024;
const MAX_ROTATED_LOGS = 3;

export function redactSensitiveText(message: string): string {
  return message
    .replace(/(Bearer\s+)[^\s]+/gi, "$1[REDACTED]")
    .replace(
      /((?:token|api[_-]?key|secret|password)\s*[:=]\s*)[^\s,;]+/gi,
      "$1[REDACTED]",
    )
    .replace(/(sk-[A-Za-z0-9_-]{12,})/g, "[REDACTED]");
}

function format(level: Level, component: string, msg: string): string {
  const entry = {
    ts: new Date().toISOString(),
    level,
    component,
    msg,
  };
  if (process.env.BAOCLAW_LOG_FORMAT === "json") return JSON.stringify(entry);
  return `[${entry.ts.replace("T", " ").slice(0, 23)}] [${level.padEnd(5)}] [${component}] ${msg}`;
}

function write(level: Level, component: string, msg: string): void {
  if (LEVELS[level] < currentLevel) return;
  const line = format(level, component, redactSensitiveText(msg));
  if (level === "ERROR") console.error(line);
  else console.log(line);
  if (logFilePath) {
    const lineBytes = Buffer.byteLength(line) + 1;
    const currentBytes = fs.existsSync(logFilePath)
      ? fs.statSync(logFilePath).size
      : 0;
    if (currentBytes + lineBytes > MAX_LOG_BYTES) {
      for (let index = MAX_ROTATED_LOGS - 1; index >= 1; index--) {
        const oldPath = `${logFilePath}.${index}`;
        const newPath = `${logFilePath}.${index + 1}`;
        if (fs.existsSync(oldPath)) fs.renameSync(oldPath, newPath);
      }
      if (fs.existsSync(logFilePath))
        fs.renameSync(logFilePath, `${logFilePath}.1`);
    }
    try {
      fs.appendFileSync(logFilePath, line + "\n", {
        encoding: "utf8",
        mode: 0o600,
      });
    } catch (error) {
      console.error(
        `Log write error: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
}

export function createLogger(component: string) {
  return {
    debug: (msg: string) => write("DEBUG", component, msg),
    info: (msg: string) => write("INFO", component, msg),
    warn: (msg: string) => write("WARN", component, msg),
    error: (msg: string) => write("ERROR", component, msg),
  };
}

export const logger = createLogger("ts-ipc");

export function setLogLevel(level: Level): void {
  currentLevel = LEVELS[level];
}

export function setLogFile(filePath: string): void {
  logFilePath = filePath;
  try {
    if (fs.existsSync(filePath)) fs.chmodSync(filePath, 0o600);
  } catch (error) {
    console.error(
      `Log permission error: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}
