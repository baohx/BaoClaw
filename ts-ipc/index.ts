export { IpcClient, type IpcClientOptions } from "./client.js";
export type { DaemonInfo, DaemonConnectorOptions } from "./daemon.js";
export {
  DaemonConnector,
  selectNewestDaemon,
  getSocketDir,
  resolveFixedSocket,
} from "./daemon.js";
export { logger, createLogger, setLogLevel, setLogFile } from "./logger.js";
export { securePrivateFile } from "./security.js";
export type {
  StreamEvent,
  StatePatch,
  QueryResult,
  ErrorInfo,
} from "./types.js";
export {
  setupStreamHandlers,
  applyStatePatch,
  applyStatePatches,
} from "./streamHandler.js";
export { startRustCore, startRustCoreWithRestart } from "./rustCore.js";
export type { RustCoreConfig, RustCoreHandle } from "./rustCore.js";
export { useRustEngine } from "./useRustEngine.js";
export type {
  Message,
  EngineState,
  UseRustEngineReturn,
} from "./useRustEngine.js";
