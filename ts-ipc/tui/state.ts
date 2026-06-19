// TUI State Management
import { TuiState, Action, Message, ContentBlock, Session, ToolProgress } from './types.js';

export const INITIAL_STATE: TuiState = {
  messages: [],
  isStreaming: false,
  streamingContent: '',
  thinkingContent: '',
  currentTools: [],
  session: null,
  input: '',
  error: null,
};

export function reducer(state: TuiState, action: Action): TuiState {
  switch (action.type) {
    case 'ADD_MESSAGE': {
      const msg = action.payload as Message;
      return {
        ...state,
        messages: [...state.messages, msg],
      };
    }

    case 'SET_STREAMING': {
      return {
        ...state,
        isStreaming: action.payload as boolean,
        streamingContent: '',
      };
    }

    case 'APPEND_STREAM': {
      const content = action.payload as string;
      return {
        ...state,
        streamingContent: state.streamingContent + content,
      };
    }

    case 'SET_THINKING': {
      return {
        ...state,
        thinkingContent: action.payload as string,
      };
    }

    case 'APPEND_THINKING': {
      const content = action.payload as string;
      return {
        ...state,
        thinkingContent: state.thinkingContent + content,
      };
    }

    case 'SET_TOOLS': {
      return {
        ...state,
        currentTools: action.payload as ToolProgress[],
      };
    }

    case 'UPDATE_TOOL': {
      const { id, update } = action.payload as { id: string; update: Partial<ToolProgress> };
      return {
        ...state,
        currentTools: state.currentTools.map(tool =>
          tool.name === id ? { ...tool, ...update } : tool
        ),
      };
    }

    case 'SET_SESSION': {
      return {
        ...state,
        session: action.payload as Session,
      };
    }

    case 'ADD_TOOL_USE': {
      const { toolName, toolId, input } = action.payload as {
        toolName: string; toolId: string; input: unknown;
      };
      const block: ContentBlock = {
        type: 'tool_use',
        content: JSON.stringify(input, null, 2),
        toolName,
        toolId,
        input,
      };
      // Append to last assistant message's content
      const messages = [...state.messages];
      if (messages.length > 0 && messages[messages.length - 1].role === 'assistant') {
        const last = messages[messages.length - 1];
        messages[messages.length - 1] = {
          ...last,
          content: [...last.content, block],
        };
      } else {
        // No ongoing assistant message, create one
        messages.push({
          id: generateId(),
          role: 'assistant',
          content: [block],
          timestamp: new Date(),
        });
      }
      return { ...state, messages };
    }

    case 'ADD_TOOL_RESULT': {
      const { toolId, output, isError } = action.payload as {
        toolId: string; output: string; isError: boolean;
      };
      const block: ContentBlock = {
        type: 'tool_result',
        content: output,
        toolId,
        isError,
      };
      const messages = [...state.messages];
      // Append to last assistant message
      if (messages.length > 0 && messages[messages.length - 1].role === 'assistant') {
        const last = messages[messages.length - 1];
        messages[messages.length - 1] = {
          ...last,
          content: [...last.content, block],
        };
      }
      return { ...state, messages };
    }

    case 'SET_INPUT': {
      return {
        ...state,
        input: action.payload as string,
      };
    }

    case 'SET_ERROR': {
      return {
        ...state,
        error: action.payload as string,
      };
    }

    case 'CLEAR_ERROR': {
      return {
        ...state,
        error: null,
      };
    }

    case 'RESET': {
      return INITIAL_STATE;
    }

    default:
      return state;
  }
}

// Helper to generate unique IDs
export function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
}

// Helper to create a user message
export function createUserMessage(content: string): Message {
  return {
    id: generateId(),
    role: 'user',
    content: [{ type: 'text', content }],
    timestamp: new Date(),
  };
}

// Helper to create an assistant message
export function createAssistantMessage(content: ContentBlock[]): Message {
  return {
    id: generateId(),
    role: 'assistant',
    content,
    timestamp: new Date(),
  };
}
