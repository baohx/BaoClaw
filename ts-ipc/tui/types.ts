// TUI Types for BaoClaw

export type ContentBlockType =
  "text" | "thinking" | "tool_use" | "tool_result" | "code";

export interface ContentBlock {
  type: ContentBlockType;
  content: string;
  language?: string;
  toolName?: string;
  toolId?: string;
  input?: unknown; // tool_use input parameters
  isError?: boolean; // tool_result error flag
}

export interface Message {
  id: string;
  role: "user" | "assistant";
  content: ContentBlock[];
  timestamp: Date;
}

export interface Session {
  id: string;
  model: string;
  status: "idle" | "streaming" | "thinking" | "error";
}

export interface ToolProgress {
  name: string;
  status: "running" | "completed" | "error";
  output?: string;
}

export type ActionType =
  | "ADD_MESSAGE"
  | "SET_STREAMING"
  | "APPEND_STREAM"
  | "SET_THINKING"
  | "APPEND_THINKING"
  | "SET_TOOLS"
  | "UPDATE_TOOL"
  | "ADD_TOOL_USE"
  | "ADD_TOOL_RESULT"
  | "SET_SESSION"
  | "SET_INPUT"
  | "SET_ERROR"
  | "CLEAR_ERROR"
  | "RESET";

export interface Action {
  type: ActionType;
  payload?: unknown;
}

export interface TuiState {
  messages: Message[];
  isStreaming: boolean;
  streamingContent: string;
  thinkingContent: string;
  currentTools: ToolProgress[];
  session: Session | null;
  input: string;
  error: string | null;
}
