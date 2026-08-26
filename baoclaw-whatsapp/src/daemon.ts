/**
 * Daemon discovery and connection, backed by the shared ts-ipc connector.
 */
import { DaemonConnector } from "../../ts-ipc/index.js";

export { type DaemonInfo, selectNewestDaemon } from "../../ts-ipc/index.js";
export { DaemonConnector } from "../../ts-ipc/index.js";

/** Preconfigured for the WhatsApp gateway's session tag. */
export function createDaemonConnector(): DaemonConnector {
  return new DaemonConnector({ sessionTag: "whatsapp" });
}
