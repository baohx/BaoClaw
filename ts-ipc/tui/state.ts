/**
 * 状态管理
 */
import type { TuiState, Action, Reducer, ContentBlock } from './types.js';

// 初始状态
export const initialState: TuiState = {
  // 连接状态
  connected: false,
  connecting: false,
  
  // 当前会话
  session: null,
  
  // 消息列表
  messages: [],
  
  // 当前流式输出
  isStreaming: false,
  streamingContent: '',
  thinkingContent: '',
  streamingMessageId: null,
  
  // 当前工具执行
  currentTools: new Map(),
  
  // 输入状态
  inputValue: '',
  inputMode: 'normal',
  
  // UI 状态
  focused: 'input',
  showHelp: false,
  showTools: false,
  showStatus: true,
  
  // 性能指标
  lastResponseTime: 0,
  ttfb: 0,
  
  // 错误
  error: null,

  // v2.0 新增
  modelList: ['claude-sonnet-4-20250514', 'claude-opus-4-20250514', 'claude-haiku-3-5-20241022'],
  currentModel: 'claude-sonnet-4-20250514',
  showModelSelector: false,
  suggestions: [] as string[],
  selectedSuggestion: 0,
  showSuggestions: false,
  searchQuery: '',
  searchResults: [] as number[],
  selectedSearchResult: 0,
  version: 'v2.0',
};

// 命令注册表
export const COMMAND_REGISTRY: { cmd: string; desc: string }[] = [
  { cmd: '/help',    desc: '显示帮助' },
  { cmd: '/status',  desc: '会话状态' },
  { cmd: '/model',   desc: '切换模型' },
  { cmd: '/clear',   desc: '清屏' },
  { cmd: '/compact', desc: '压缩上下文' },
  { cmd: '/sessions',desc: '会话列表' },
  { cmd: '/tools',   desc: '工具面板' },
  { cmd: '/memory',  desc: '记忆管理' },
  { cmd: '/cron',    desc: '定时任务' },
  { cmd: '/git',     desc: 'Git 状态' },
  { cmd: '/search',  desc: '搜索消息' },
  { cmd: '/skills',  desc: '技能列表' },
  { cmd: '/gateway', desc: '网关状态' },
];

// Reducer
export const reducer: Reducer = (state: TuiState, action: Action): TuiState => {
  switch (action.type) {
    case 'SET_CONNECTED':
      return { ...state, connected: action.payload as boolean };
      
    case 'SET_CONNECTING':
      return { ...state, connecting: action.payload as boolean };
      
    case 'SET_SESSION':
      return { 
        ...state, 
        session: action.payload as TuiState['session'],
      };
      
    case 'ADD_MESSAGE': {
      const newMsg = action.payload as TuiState['messages'][0];
      // 如果是助手消息且正在流式输出，将流式内容赋给它
      if (newMsg.role === 'assistant' && state.streamingContent) {
        newMsg.content = [{ type: 'text', text: state.streamingContent }];
      }
      return {
        ...state,
        messages: [...state.messages, newMsg],
      };
    }
      
    case 'UPDATE_MESSAGE':
      const { id, updates } = action.payload as { id: string; updates: Partial<TuiState['messages'][0]> };
      return {
        ...state,
        messages: state.messages.map((msg) =>
          msg.id === id ? { ...msg, ...updates } : msg
        ),
      };
      
    case 'SET_STREAMING':
      const msgId = `assistant-${Date.now()}`;
      return {
        ...state,
        isStreaming: action.payload as boolean,
        streamingContent: '',
        thinkingContent: '',
        streamingMessageId: action.payload ? msgId : null,
        // 不添加到 messages——由 StreamOutput 独占渲染，CLEAR_STREAM 时才归档
      };
      
    case 'APPEND_STREAM': {
      const newContent = state.streamingContent + (action.payload as string);
      
      // 如果还没有流式消息ID，自动创建一个
      if (!state.streamingMessageId) {
        return {
          ...state,
          isStreaming: true,
          streamingContent: newContent,
          streamingMessageId: `assistant-${Date.now()}`,
        };
      }
      
      return {
        ...state,
        isStreaming: true,
        streamingContent: newContent,
        // 不更新 messages——StreamOutput 独占渲染流式内容
      };
    }
      
    case 'APPEND_THINKING':
      return {
        ...state,
        isStreaming: true,
        thinkingContent: state.thinkingContent + (action.payload as string),
      };
      
    case 'CLEAR_STREAM': {
      const msgId = state.streamingMessageId || `assistant-${Date.now()}`;
      
      // 组装 content blocks：thinking → tools → text
      const blocks: ContentBlock[] = [];
      
      // 💭 思考块
      if (state.thinkingContent.trim()) {
        blocks.push({ type: 'thinking', text: state.thinkingContent });
      }
      
      // 🔧 工具块
      if (state.currentTools.size > 0) {
        for (const [id, tool] of state.currentTools) {
          blocks.push({
            type: 'tool_use',
            toolName: tool.name,
            input: tool.input,
            output: tool.output,
            isError: tool.status === 'error',
          });
        }
      }
      
      // ✦ 回复块
      if (state.streamingContent.trim()) {
        blocks.push({ type: 'text', text: state.streamingContent });
      }
      
      return {
        ...state,
        isStreaming: false,
        streamingContent: '',
        thinkingContent: '',
        currentTools: new Map(),
        streamingMessageId: null,
        messages: blocks.length > 0
          ? [...state.messages, {
              id: msgId,
              role: 'assistant' as const,
              timestamp: new Date().toISOString(),
              content: blocks,
            }]
          : state.messages,
      };
    }
      
    case 'SET_TOOL_STATE':
      const { id: toolId, state: toolState } = action.payload as {
        id: string;
        state: Partial<TuiState['currentTools'] extends Map<string, infer V> ? V : never>;
      };
      const newTools = new Map(state.currentTools);
      const existing = newTools.get(toolId) || { name: '', status: 'pending' };
      newTools.set(toolId, { ...existing, ...toolState } as TuiState['currentTools'] extends Map<string, infer V> ? V : never);
      return { ...state, currentTools: newTools };
      
    case 'SET_INPUT_VALUE':
      return { ...state, inputValue: action.payload as string };
      
    case 'SET_INPUT_MODE':
      return { ...state, inputMode: action.payload as TuiState['inputMode'] };
      
    case 'SET_FOCUSED':
      return { ...state, focused: action.payload as TuiState['focused'] };
      
    case 'TOGGLE_HELP':
      return { ...state, showHelp: !state.showHelp };
      
    case 'TOGGLE_TOOLS':
      return { ...state, showTools: !state.showTools };
      
    case 'SET_ERROR':
      return { ...state, error: action.payload as string };
      
    case 'CLEAR_ERROR':
      return { ...state, error: null };
      
    case 'SET_MODEL_LIST':
      return { ...state, modelList: action.payload as string[] };
      
    case 'SET_CURRENT_MODEL':
      return { ...state, currentModel: action.payload as string, showModelSelector: false };
      
    case 'SHOW_MODEL_SELECTOR':
      return { ...state, showModelSelector: action.payload as boolean };
      
    case 'SET_SUGGESTIONS':
      return { ...state, suggestions: action.payload as string[], selectedSuggestion: 0, showSuggestions: (action.payload as string[]).length > 0 };
      
    case 'SET_SELECTED_SUGGESTION':
      return { ...state, selectedSuggestion: action.payload as number };
      
    case 'SHOW_SUGGESTIONS':
      return { ...state, showSuggestions: action.payload as boolean };
      
    case 'SET_SEARCH_QUERY':
      return { ...state, searchQuery: action.payload as string };
      
    case 'SET_SEARCH_RESULTS': {
      const results = action.payload as { results: number[]; selected: number };
      return { ...state, searchResults: results.results, selectedSearchResult: results.selected };
    }
      
    case 'SET_SELECTED_SEARCH_RESULT':
      return { ...state, selectedSearchResult: action.payload as number };

    default:
      return state;
  }
};
