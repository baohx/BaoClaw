/**
 * TUI 类型定义
 */

// 消息角色
export type MessageRole = 'user' | 'assistant' | 'system' | 'tool';

// 消息内容块类型
export type ContentBlockType = 'text' | 'thinking' | 'tool_use' | 'tool_result' | 'code';

// 内容块
export interface ContentBlock {
  type: ContentBlockType;
  text?: string;
  toolName?: string;
  toolUseId?: string;
  input?: Record<string, unknown>;
  output?: unknown;
  language?: string;
  isError?: boolean;
}

// 消息
export interface Message {
  id: string;
  role: MessageRole;
  timestamp: string;
  content: ContentBlock[];
  tokens?: {
    input: number;
    output: number;
  };
  cost?: number;
  duration?: number;
}

// 会话状态
export interface SessionState {
  id: string;
  cwd: string;
  model: string;
  messageCount: number;
  totalTokens: number;
  totalCost: number;
  contextWindow: number;
  contextUsage: number; // 百分比
}

// 工具状态
export interface ToolState {
  name: string;
  status: 'pending' | 'running' | 'success' | 'error';
  startTime?: number;
  endTime?: number;
  input?: Record<string, unknown>;
  output?: unknown;
}

// TUI 全局状态
export interface TuiState {
  // 连接状态
  connected: boolean;
  connecting: boolean;
  
  // 当前会话
  session: SessionState | null;
  
  // 消息列表
  messages: Message[];
  
  // 当前流式输出
  isStreaming: boolean;
  streamingContent: string;
  thinkingContent: string;
  streamingMessageId: string | null; // 当前流式消息ID
  
  // 当前工具执行
  currentTools: Map<string, ToolState>;
  
  // 输入状态
  inputValue: string;
  inputMode: 'normal' | 'multiline' | 'command';
  
  // UI 状态
  focused: 'input' | 'messages' | 'help';
  showHelp: boolean;
  showTools: boolean;
  showStatus: boolean;
  
  // 性能指标
  lastResponseTime: number;
  ttfb: number; // Time to first byte
  
  // v2 新增
  modelList: string[];
  currentModel: string;
  showModelSelector: boolean;
  suggestions: string[];
  selectedSuggestion: number;
  showSuggestions: boolean;
  searchQuery: string;
  searchResults: number[];
  selectedSearchResult: number;
  version: string;
  
  // 错误
  error: string | null;
}

// IPC 事件类型
export type IpcEventType = 
  | 'assistant_chunk'
  | 'thinking_chunk'
  | 'tool_use'
  | 'tool_result'
  | 'turn_start'
  | 'turn_end'
  | 'progress'
  | 'permission_request'
  | 'result'
  | 'error'
  | 'model_fallback';

// IPC 事件
export interface IpcEvent {
  type: IpcEventType;
  [key: string]: unknown;
}

// 命令定义
export interface Command {
  name: string;
  alias?: string[];
  description: string;
  usage?: string;
  handler: (args: string, state: TuiState, dispatch: React.Dispatch<Action>) => Promise<void> | void;
}

// Action 类型
export type ActionType =
  | 'SET_CONNECTED'
  | 'SET_CONNECTING'
  | 'SET_SESSION'
  | 'ADD_MESSAGE'
  | 'UPDATE_MESSAGE'
  | 'SET_STREAMING'
  | 'APPEND_STREAM'
  | 'APPEND_THINKING'
  | 'CLEAR_STREAM'
  | 'SET_TOOL_STATE'
  | 'SET_INPUT_VALUE'
  | 'SET_INPUT_MODE'
  | 'SET_FOCUSED'
  | 'TOGGLE_HELP'
  | 'TOGGLE_TOOLS'
  | 'SET_ERROR'
  | 'CLEAR_ERROR'
  | 'SET_MODEL_LIST'
  | 'SET_CURRENT_MODEL'
  | 'SHOW_MODEL_SELECTOR'
  | 'SET_SUGGESTIONS'
  | 'SET_SELECTED_SUGGESTION'
  | 'SHOW_SUGGESTIONS'
  | 'SET_SEARCH_QUERY'
  | 'SET_SEARCH_RESULTS'
  | 'SET_SELECTED_SEARCH_RESULT';

// Action
export interface Action {
  type: ActionType;
  payload?: unknown;
}

// Reducer
export type Reducer = (state: TuiState, action: Action) => TuiState;
