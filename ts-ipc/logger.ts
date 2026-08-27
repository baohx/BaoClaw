import * as fs from "fs";

const LEVELS = { DEBUG: 0, INFO: 1, WARN: 2, ERROR: 3 } as const;
type Level = keyof typeof LEVELS;

let currentLevel: number = LEVELS.INFO;
let logStream: fs.WriteStream | null = null;

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
  const line = format(level, component, msg);
  if (level === "ERROR") console.error(line);
  else console.log(line);
  logStream?.write(line + "\n");
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
  logStream = fs.createWriteStream(filePath, { flags: "a" });
  logStream.on("error", (err) =>
    logger.error(`Log write error: ${err.message}`),
  );
}
