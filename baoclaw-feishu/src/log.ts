/**
 * Structured logging for BaoClaw Feishu Gateway.
 * Timestamps, log levels, and optional file output.
 */
import * as fs from 'fs';

const LOG_LEVELS = { DEBUG: 0, INFO: 1, WARN: 2, ERROR: 3 } as const;
type Level = keyof typeof LOG_LEVELS;

let currentLevel: number = LOG_LEVELS.INFO;
let logStream: fs.WriteStream | null = null;
let pid: number = process.pid;

function timestamp(): string {
  return new Date().toISOString().replace('T', ' ').slice(0, 23);
}

function fmt(level: Level, msg: string): string {
  return `[${timestamp()}] [${level.padEnd(5)}] [pid=${pid}] ${msg}`;
}

function log(level: Level, msg: string): void {
  if (LOG_LEVELS[level] < currentLevel) return;
  const line = fmt(level, msg);
  if (level === 'ERROR') console.error(line);
  else console.log(line);
  if (logStream) logStream.write(line + '\n');
}

export const logger = {
  debug: (msg: string) => log('DEBUG', msg),
  info:  (msg: string) => log('INFO', msg),
  warn:  (msg: string) => log('WARN', msg),
  error: (msg: string) => log('ERROR', msg),
};

export function setLogLevel(level: Level): void {
  currentLevel = LOG_LEVELS[level];
}

export function setLogFile(path: string): void {
  logStream = fs.createWriteStream(path, { flags: 'a' });
  logStream.on('error', (e) => console.error('[log] write error:', e.message));
}

export function setPid(p: number): void {
  pid = p;
}
