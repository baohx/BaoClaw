import { useState, useEffect, useCallback, useRef } from 'react';
import { IpcClient } from './client';
import { StatePatch, QueryResult, ErrorInfo } from './types';
import { setupStreamHandlers, applyStatePatches, StreamHandlerManager } from './streamHandler';

export interface Message {
  uuid: string;
  type: 'user' | 'assistant' | 'tool_use' | 'tool_result';
  content: string;
  toolName?: string;
  toolUseId?: string;
  isError?: boolean;
}

export interface EngineState {
  messages: Message[];
  isProcessing: boolean;
  error: ErrorInfo | null;
  lastResult: QueryResult | null;
  coreState: Record<string, unknown>;
}

export interface UseRustEngineReturn {
  state: EngineState;
  submitMessage: (prompt: string) => Promise<void>;
  abort: () => Promise<void>;
  isConnected: boolean;
}

/**
 * React hook that manages communication with the Rust core engine.
 * Handles stream events, state patches, and message accumulation.
 */
export function useRustEngine(client: IpcClient | null): UseRustEngineReturn {
  // State
  const [messages, setMessages] = useState<Message[]>([]);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<ErrorInfo | null>(null);
  const [lastResult, setLastResult] = useState<QueryResult | null>(null);
  const [coreState, setCoreState] = useState<Record<string, unknown>>({});
  const [isConnected, setIsConnected] = useState(false);

  // Refs for current assistant message accumulation
  const currentAssistantText = useRef('');
  const handlerManager = useRef<StreamHandlerManager | null>(null);

  // Set up stream handlers when client changes
  useEffect(() => {
    if (!client) {
      setIsConnected(false);
      return;
    }

    setIsConnected(true);
    const manager = setupStreamHandlers(client);
    handlerManager.current = manager;

    // Handle assistant text chunks — accumulate and update in-place
    manager.onEventType('assistant_chunk', (event) => {
      currentAssistantText.current += event.content;
      // Update the last assistant message in-place
      setMessages(prev => {
        const last = prev[prev.length - 1];
        if (last && last.type === 'assistant') {
          return [
            ...prev.slice(0, -1),
            { ...last, content: currentAssistantText.current },
          ];
        }
        // Create new assistant message
        return [
          ...prev,
          {
            uuid: crypto.randomUUID(),
            type: 'assistant' as const,
            content: currentAssistantText.current,
          },
        ];
      });
    });

    manager.onEventType('tool_use', (event) => {
      setMessages(prev => [
        ...prev,
        {
          uuid: crypto.randomUUID(),
          type: 'tool_use' as const,
          content: JSON.stringify(event.input),
          toolName: event.toolName,
          toolUseId: event.toolUseId,
        },
      ]);
    });

    manager.onEventType('tool_result', (event) => {
      setMessages(prev => [
        ...prev,
        {
          uuid: crypto.randomUUID(),
          type: 'tool_result' as const,
          content: typeof event.output === 'string' ? event.output : JSON.stringify(event.output),
          toolUseId: event.toolUseId,
          isError: event.isError,
        },
      ]);
    });

    manager.onEventType('result', (event) => {
      setLastResult(event.result);
      setIsProcessing(false);
    });

    manager.onEventType('error', (event) => {
      setError(event.error);
      setIsProcessing(false);
    });

    // Handle state patches
    manager.onStatePatch((patches: StatePatch[]) => {
      setCoreState(prev => {
        const next = { ...prev };
        applyStatePatches(next, patches);
        return next;
      });
    });

    return () => {
      manager.dispose();
      handlerManager.current = null;
    };
  }, [client]);

  // Submit a message
  const submitMessage = useCallback(async (prompt: string) => {
    if (!client) throw new Error('Not connected');

    // Add user message
    setMessages(prev => [
      ...prev,
      {
        uuid: crypto.randomUUID(),
        type: 'user' as const,
        content: prompt,
      },
    ]);

    // Reset state for new query
    currentAssistantText.current = '';
    setIsProcessing(true);
    setError(null);
    setLastResult(null);

    // Send to Rust core
    await client.request('submitMessage', { prompt });
  }, [client]);

  // Abort current query
  const abort = useCallback(async () => {
    if (!client) return;
    await client.request('abort');
  }, [client]);

  return {
    state: {
      messages,
      isProcessing,
      error,
      lastResult,
      coreState,
    },
    submitMessage,
    abort,
    isConnected,
  };
}
