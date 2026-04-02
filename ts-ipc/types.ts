export type StreamEvent =
  | { type: 'assistant_chunk'; content: string; toolUseId?: string }
  | { type: 'tool_use'; toolName: string; input: Record<string, unknown>; toolUseId: string }
  | { type: 'tool_result'; toolUseId: string; output: unknown; isError: boolean }
  | { type: 'permission_request'; toolName: string; input: Record<string, unknown>; toolUseId: string }
  | { type: 'progress'; toolUseId: string; data: unknown }
  | { type: 'state_update'; patch: StatePatch }
  | { type: 'result'; result: QueryResult }
  | { type: 'error'; error: ErrorInfo };

export interface StatePatch {
  path: string;
  op: 'replace' | 'add' | 'remove';
  value?: unknown;
}

export interface QueryResult {
  status: 'complete' | 'max_turns' | 'aborted' | 'error';
  text?: string;
  stopReason?: string;
  totalCostUsd?: number;
  usage?: { inputTokens: number; outputTokens: number };
}

export interface ErrorInfo {
  code: number;
  message: string;
  data?: unknown;
}
