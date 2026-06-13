// IPC Integration for BaoClaw TUI
import { IpcClient } from '../client.js';
import { Action } from './types.js';

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
export async function createIpcConnection(config: IpcConfig): Promise<IpcClient> {
  const client = new IpcClient();
  await client.connect(config.socketPath);
  
  // Send initialize message to register as a client
  // This is required by the backend
  try {
    await client.request('initialize', {
      cwd: config.cwd || process.cwd(),
      model: config.model,
      settings: {},
      shared_session_id: 'tui',
    }, 10000);
  } catch (err) {
    // Log but continue - some backends may not require initialize
    console.log('Initialize response received');
  }
  
  return client;
}

// Subscribe to IPC events and dispatch actions
// The backend sends "stream/event" notifications with EngineEvent types
export function subscribeToEvents(
  client: IpcClient,
  dispatch: React.Dispatch<Action>
): () => void {
  const handlers: Array<() => void> = [];

  // Main stream/event handler - handles all EngineEvent types
  const unsubStreamEvent = client.onNotification('stream/event', (params) => {
    const p = params as { type: string; [key: string]: unknown };
    
    switch (p.type) {
      case 'assistant_chunk': {
        // { type: "assistant_chunk", content: string, tool_use_id?: string }
        const content = p.content as string;
        dispatch({ type: 'APPEND_STREAM', payload: content });
        break;
      }
      
      case 'thinking_chunk': {
        // { type: "thinking_chunk", content: string }
        const content = p.content as string;
        dispatch({ type: 'APPEND_THINKING', payload: content });
        break;
      }
      
      case 'tool_use': {
        // { type: "tool_use", tool_name: string, input: object, tool_use_id: string }
        // Could dispatch to add tool to list
        break;
      }
      
      case 'tool_result': {
        // { type: "tool_result", tool_use_id: string, output: object, is_error: bool }
        // Could dispatch to update tool status
        break;
      }
      
      case 'result': {
        // { type: "result", status: string, usage: object }
        // Stream complete - handled in App component
        dispatch({ type: 'SET_STREAMING', payload: false });
        break;
      }
      
      case 'error': {
        // { type: "error", message: string }
        const message = p.message || p.error?.message || 'Unknown error';
        dispatch({ type: 'SET_ERROR', payload: String(message) });
        break;
      }
    }
  });
  handlers.push(unsubStreamEvent);

  // Error notification (direct)
  const unsubError = client.onNotification('error', (params) => {
    const p = params as { message: string };
    dispatch({ type: 'SET_ERROR', payload: p.message });
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
  content: string
): Promise<void> {
  await client.request('submitMessage', {
    prompt: { content },
    uuid: null,
    attachments: null,
  });
}
