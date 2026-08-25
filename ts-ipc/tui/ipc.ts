// IPC Integration for BaoClaw TUI
import { IpcClient } from "../client.js";
import { Action } from "./types.js";

export type IpcEventHandler = (event: IpcEvent) => void;

export interface IpcEvent {
  type: string;
  [key: string]: unknown;
}

export interface IpcConfig {
  socketPath: string;
  cwd?: string;
  model?: string;
}

// Create IPC client and connect
export async function createIpcConnection(
  config: IpcConfig,
): Promise<IpcClient> {
  const client = new IpcClient();
  await client.connect(config.socketPath);

  // Send initialize message to register as a client
  // This is required by the backend
  try {
    await client.request(
      "initialize",
      {
        cwd: config.cwd || process.cwd(),
        model: config.model,
        settings: {},
        shared_session_id: "tui",
      },
      10000,
    );
  } catch (err) {
    // Log but continue - some backends may not require initialize
    console.log("Initialize response received");
  }

  return client;
}

// Subscribe to IPC events and dispatch actions
// The backend sends "stream/event" notifications with EngineEvent types
export function subscribeToEvents(
  client: IpcClient,
  dispatch: React.Dispatch<Action>,
): () => void {
  const handlers: Array<() => void> = [];

  // ── Stream chunk batching ──────────────────────────────────────────
  // assistant/thinking chunks can arrive at token rate (dozens per second).
  // Dispatching each one triggers a full Ink re-render; instead we buffer
  // chunks and flush at most once per FLUSH_INTERVAL_MS. A trailing timer
  // guarantees the final buffered text is never lost.
  const FLUSH_INTERVAL_MS = 70;
  let streamBuf = "";
  let thinkingBuf = "";
  let flushTimer: ReturnType<typeof setTimeout> | null = null;

  const flushBuffers = () => {
    if (streamBuf) {
      dispatch({ type: "APPEND_STREAM", payload: streamBuf });
      streamBuf = "";
    }
    if (thinkingBuf) {
      dispatch({ type: "APPEND_THINKING", payload: thinkingBuf });
      thinkingBuf = "";
    }
    flushTimer = null;
  };

  const scheduleFlush = () => {
    if (flushTimer === null) {
      flushTimer = setTimeout(flushBuffers, FLUSH_INTERVAL_MS);
    }
  };

  // Flush immediately when the turn ends so no trailing text is stuck.
  const flushNow = () => {
    if (flushTimer !== null) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    flushBuffers();
  };

  // Main stream/event handler - handles all EngineEvent types
  const unsubStreamEvent = client.onNotification("stream/event", (params) => {
    const p = params as { type: string; [key: string]: unknown };

    switch (p.type) {
      case "assistant_chunk": {
        // { type: "assistant_chunk", content: string, tool_use_id?: string }
        streamBuf += p.content as string;
        scheduleFlush();
        break;
      }

      case "thinking_chunk": {
        // { type: "thinking_chunk", content: string }
        thinkingBuf += p.content as string;
        scheduleFlush();
        break;
      }

      case "tool_use": {
        // { type: "tool_use", tool_name: string, input: object, tool_use_id: string }
        const toolName = (p.tool_name as string) || "tool";
        const toolId = (p.tool_use_id as string) || "";
        const input = p.input ?? {};
        dispatch({
          type: "ADD_TOOL_USE",
          payload: { toolName, toolId, input },
        });
        break;
      }

      case "tool_result": {
        // { type: "tool_result", tool_use_id: string, output: object, is_error: bool }
        const toolId = (p.tool_use_id as string) || "";
        const output =
          typeof p.output === "string"
            ? p.output
            : JSON.stringify(p.output, null, 2);
        const isError = p.is_error === true;
        dispatch({
          type: "ADD_TOOL_RESULT",
          payload: { toolId, output, isError },
        });
        break;
      }

      case "result": {
        // { type: "result", status: string, usage: object }
        // Stream complete - handled in App component
        flushNow();
        dispatch({ type: "SET_STREAMING", payload: false });
        break;
      }

      case "error": {
        // { type: "error", message: string }
        flushNow();
        const errorParams = p as {
          message?: unknown;
          error?: { message?: unknown };
        };
        const message =
          errorParams.message || errorParams.error?.message || "Unknown error";
        dispatch({ type: "SET_ERROR", payload: String(message) });
        break;
      }
    }
  });
  handlers.push(unsubStreamEvent);

  // Error notification (direct)
  const unsubError = client.onNotification("error", (params) => {
    const p = params as { message: string };
    dispatch({ type: "SET_ERROR", payload: p.message });
  });
  handlers.push(unsubError);

  // Return unsubscribe all
  return () => {
    for (const unsub of handlers) {
      unsub();
    }
  };
}

// Send a message to the backend using submitMessage
export async function sendMessage(
  client: IpcClient,
  content: string,
): Promise<void> {
  await client.request("submitMessage", {
    prompt: content, // string (daemon expects prompt.as_str())
    uuid: null,
    attachments: null,
  });
}
