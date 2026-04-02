import { IpcClient } from './client';
import { StreamEvent, StatePatch } from './types';

type StreamEventHandler = (event: StreamEvent) => void;
type StatePatchHandler = (patches: StatePatch[]) => void;

export interface StreamHandlerManager {
  /** Subscribe to all stream events */
  onStreamEvent(handler: StreamEventHandler): () => void;

  /** Subscribe to state patches */
  onStatePatch(handler: StatePatchHandler): () => void;

  /** Subscribe to a specific stream event type */
  onEventType<T extends StreamEvent['type']>(
    type: T,
    handler: (event: Extract<StreamEvent, { type: T }>) => void,
  ): () => void;

  /** Unsubscribe all handlers */
  dispose(): void;
}

/**
 * Parse a JSON Pointer path (RFC 6901) into an array of unescaped segments.
 * e.g. "/tasks/b12345678" → ["tasks", "b12345678"]
 *      "/foo~1bar/baz~0qux" → ["foo/bar", "baz~qux"]
 */
function parseJsonPointer(path: string): string[] | null {
  if (path === '') return [];
  if (!path.startsWith('/')) return null;

  return path
    .slice(1)
    .split('/')
    .map((seg) => seg.replace(/~1/g, '/').replace(/~0/g, '~'));
}

/**
 * Apply a StatePatch to a state object using JSON Pointer paths.
 * @param state - The current state object (will be mutated)
 * @param patch - The patch to apply
 * @returns true if the patch was applied successfully, false otherwise
 */
export function applyStatePatch(state: Record<string, unknown>, patch: StatePatch): boolean {
  const segments = parseJsonPointer(patch.path);
  if (segments === null || segments.length === 0) return false;

  const parentSegments = segments.slice(0, -1);
  const lastSegment = segments[segments.length - 1];

  // Traverse to the parent object
  let current: unknown = state;
  for (const seg of parentSegments) {
    if (current === null || current === undefined || typeof current !== 'object') {
      return false;
    }
    current = (current as Record<string, unknown>)[seg];
  }

  if (current === null || current === undefined || typeof current !== 'object') {
    return false;
  }

  const parent = current as Record<string, unknown>;

  switch (patch.op) {
    case 'replace':
    case 'add':
      parent[lastSegment] = patch.value;
      return true;

    case 'remove':
      if (!(lastSegment in parent)) return false;
      delete parent[lastSegment];
      return true;

    default:
      return false;
  }
}

/**
 * Apply multiple patches to a state object.
 * @returns true if all patches were applied successfully
 */
export function applyStatePatches(
  state: Record<string, unknown>,
  patches: StatePatch[],
): boolean {
  let allSuccess = true;
  for (const patch of patches) {
    if (!applyStatePatch(state, patch)) {
      allSuccess = false;
    }
  }
  return allSuccess;
}

/**
 * Sets up stream event and state patch handlers on an IPC client.
 * Returns an object with methods to subscribe to specific event types
 * and an unsubscribe-all function.
 */
export function setupStreamHandlers(client: IpcClient): StreamHandlerManager {
  const streamEventHandlers = new Set<StreamEventHandler>();
  const statePatchHandlers = new Set<StatePatchHandler>();
  const typedHandlers = new Map<string, Set<(event: StreamEvent) => void>>();
  const unsubscribers: (() => void)[] = [];

  // Listen for stream/event notifications
  const unsubStream = client.onNotification('stream/event', (params: unknown) => {
    const event = params as StreamEvent;
    if (!event || typeof event !== 'object' || !('type' in event)) return;

    // Dispatch to all-events handlers
    for (const handler of streamEventHandlers) {
      handler(event);
    }

    // Dispatch to type-specific handlers
    const typed = typedHandlers.get(event.type);
    if (typed) {
      for (const handler of typed) {
        handler(event);
      }
    }
  });
  unsubscribers.push(unsubStream);

  // Listen for state/patch notifications
  const unsubPatch = client.onNotification('state/patch', (params: unknown) => {
    const data = params as { patches?: StatePatch[] };
    if (!data || !Array.isArray(data.patches)) return;

    for (const handler of statePatchHandlers) {
      handler(data.patches);
    }
  });
  unsubscribers.push(unsubPatch);

  return {
    onStreamEvent(handler: StreamEventHandler): () => void {
      streamEventHandlers.add(handler);
      return () => {
        streamEventHandlers.delete(handler);
      };
    },

    onStatePatch(handler: StatePatchHandler): () => void {
      statePatchHandlers.add(handler);
      return () => {
        statePatchHandlers.delete(handler);
      };
    },

    onEventType<T extends StreamEvent['type']>(
      type: T,
      handler: (event: Extract<StreamEvent, { type: T }>) => void,
    ): () => void {
      let handlers = typedHandlers.get(type);
      if (!handlers) {
        handlers = new Set();
        typedHandlers.set(type, handlers);
      }
      const wrappedHandler = handler as (event: StreamEvent) => void;
      handlers.add(wrappedHandler);
      return () => {
        handlers!.delete(wrappedHandler);
        if (handlers!.size === 0) {
          typedHandlers.delete(type);
        }
      };
    },

    dispose(): void {
      streamEventHandlers.clear();
      statePatchHandlers.clear();
      typedHandlers.clear();
      for (const unsub of unsubscribers) {
        unsub();
      }
      unsubscribers.length = 0;
    },
  };
}
