export { IpcClient, type IpcClientOptions } from "./client.js";
export type { DaemonInfo, DaemonConnectorOptions } from "./daemon.js";
export {
  DaemonConnector,
  selectNewestDaemon,
  getSocketDir,
  resolveFixedSocket,
} from "./daemon.js";
export { StreamEvent, StatePatch, QueryResult, ErrorInfo } from "./types.js";
export {
  setupStreamHandlers,
  applyStatePatch,
  applyStatePatches,
} from "./streamHandler.js";
export {
  startRustCore,
  startRustCoreWithRestart,
  RustCoreConfig,
  RustCoreHandle,
} from "./rustCore.js";
export {
  useRustEngine,
  Message,
  EngineState,
  UseRustEngineReturn,
} from "./useRustEngine.js";
