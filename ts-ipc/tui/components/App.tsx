/**
 * 主应用组件
 * 
 * 极客禅宗布局：
 * ┌────────────────────────────────────┐
 * │  ◉ Session · Model · Tokens        │  状态栏
 * ├────────────────────────────────────┤
 * │                                    │
 * │  消息区域（滚动）                    │  消息区
 * │  · 用户消息                         │
 * │  · 思考过程（可折叠）                │
 * │  · 工具执行（可折叠）                │
 * │  · 助手回复                         │
 * │                                    │
 * ├────────────────────────────────────┤
 * │  ○ thinking...                     │  流式输出
 * │  ○ tool_name...                    │
 * ├────────────────────────────────────┤
 * │  ❯ _                               │  输入区
 * └────────────────────────────────────┘
 */
import React, { useReducer, useEffect, useCallback, useRef } from 'react';
import { Box, useApp, useInput, useStdout } from 'ink';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { StatusBar } from './StatusBar.js';
import { MessageList } from './MessageList.js';
import { StreamOutput } from './StreamOutput.js';
import { InputArea } from './InputArea.js';
import { HelpOverlay } from './HelpOverlay.js';
import { ToolsPanel } from './ToolsPanel.js';
import { ShortcutBar } from './ShortcutBar.js';
import { ModelSelector } from './ModelSelector.js';
import { SearchOverlay } from './SearchOverlay.js';
import { initialState, reducer, COMMAND_REGISTRY } from '../state.js';
import { IpcClientImpl } from '../ipc.js';
import { layout } from '../theme.js';
import type { TuiState, Action } from '../types.js';

interface AppProps {
  socketPath?: string;
}

export function App({ socketPath }: AppProps) {
  const { exit } = useApp();
  const { stdout } = useStdout();
  const [state, dispatch] = useReducer(reducer, initialState);
  
  // IPC 客户端引用
  const ipcRef = useRef<IpcClientImpl | null>(null);
  
  // 终端尺寸
  const width = stdout.columns || 80;
  const height = stdout.rows || 24;
  
  // 连接 IPC
  useEffect(() => {
    async function connect() {
      dispatch({ type: 'SET_CONNECTING', payload: true });
      
      try {
        const client = new IpcClientImpl();
        await client.connect(socketPath || '/tmp/baoclaw-sockets/default.sock');
        ipcRef.current = client;
        
        // 初始化会话
        const initResult = await client.request<{
          session_id: string;
          model?: string;
          reconnected?: boolean;
          message_count?: number;
        }>('initialize', {
          cwd: process.cwd(),
          settings: {},
          shared_session_id: 'default',
        });
        
        // 读取配置文件获取模型
        let configModel = 'unknown';
        try {
          const configPath = path.join(os.homedir(), '.baoclaw', 'config.json');
          const configData = fs.readFileSync(configPath, 'utf-8');
          const config = JSON.parse(configData);
          configModel = config.model || 'unknown';
        } catch {
          configModel = process.env.ANTHROPIC_MODEL || 'unknown';
        }
        
        dispatch({ type: 'SET_CONNECTED', payload: true });
        dispatch({ 
          type: 'SET_SESSION', 
          payload: {
            id: initResult.session_id,
            model: configModel,
            cwd: process.cwd(),
            messageCount: initResult.message_count || 0,
            totalTokens: 0,
            totalCost: 0,
            contextWindow: 200000,
            contextUsage: 0,
          }
        });
        
        // 注册事件处理
        client.onEvent((event) => {
          handleIpcEvent(event, dispatch);
        });
      } catch (error) {
        dispatch({ type: 'SET_ERROR', payload: String(error) });
      }
      
      dispatch({ type: 'SET_CONNECTING', payload: false });
    }
    
    connect();
    
    return () => {
      ipcRef.current?.disconnect();
    };
  }, [socketPath]);
  
  // 键盘输入处理
  useInput(useCallback((input, key) => {
    // Ctrl+C 退出
    if (key.ctrl && input === 'c') {
      exit();
      return;
    }
    
    // Ctrl+R 搜索
    if (key.ctrl && input === 'r' && state.focused === 'input') {
      dispatch({ type: 'SET_SEARCH_RESULTS', payload: { results: [], selected: 0 } });
      dispatch({ type: 'SET_SEARCH_QUERY', payload: '' });
      return;
    }
    
    // 切换帮助
    if (input === '?' && state.focused === 'input') {
      dispatch({ type: 'TOGGLE_HELP' });
      return;
    }
    
    // 切换工具面板
    if (input === 't' && state.focused === 'input' && !state.inputValue) {
      dispatch({ type: 'TOGGLE_TOOLS' });
      return;
    }
    
    // ESC 关闭覆盖层
    if (key.escape) {
      if (state.showHelp) dispatch({ type: 'TOGGLE_HELP' });
      if (state.showTools) dispatch({ type: 'TOGGLE_TOOLS' });
      if (state.showModelSelector) dispatch({ type: 'SHOW_MODEL_SELECTOR', payload: false });
      if (state.searchQuery !== '') dispatch({ type: 'SET_SEARCH_QUERY', payload: '' });
      return;
    }
    
    // 模型选择器键盘导航
    if (state.showModelSelector) {
      if (key.return && state.modelList.length > 0) {
        const model = state.modelList[state.selectedSuggestion];
        ipcRef.current?.request('setModel', { model }).catch(() => {});
        dispatch({ type: 'SET_CURRENT_MODEL', payload: model });
        return;
      }
      if (key.upArrow) {
        dispatch({
          type: 'SET_SELECTED_SUGGESTION',
          payload: Math.max(0, state.selectedSuggestion - 1),
        });
        return;
      }
      if (key.downArrow) {
        dispatch({
          type: 'SET_SELECTED_SUGGESTION',
          payload: Math.min(state.modelList.length - 1, state.selectedSuggestion + 1),
        });
        return;
      }
    }
  }, [state.focused, state.inputValue, state.showHelp, state.showTools, state.showModelSelector, state.searchQuery, state.modelList, state.selectedSuggestion, exit]));
  
  // 发送消息
  const sendMessage = useCallback(async (text: string) => {
    if (!ipcRef.current || !text.trim()) return;
    
    // 处理斜杠命令——无参数命令
    const trimmed = text.trim();
    if (trimmed === '/model') {
      dispatch({ type: 'SHOW_MODEL_SELECTOR', payload: true });
      return;
    }
    if (trimmed === '/clear') {
      dispatch({ type: 'SET_STREAMING', payload: false });
      return;
    }
    
    // 添加用户消息
    dispatch({
      type: 'ADD_MESSAGE',
      payload: {
        id: `user-${Date.now()}`,
        role: 'user',
        timestamp: new Date().toISOString(),
        content: [{ type: 'text', text }],
      },
    });
    
    // 清空输入
    dispatch({ type: 'SET_INPUT_VALUE', payload: '' });
    // 关闭补全弹窗
    dispatch({ type: 'SHOW_SUGGESTIONS', payload: false });
    
    // 发送到 IPC
    try {
      await ipcRef.current.request('submitMessage', { prompt: text });
    } catch (error) {
      dispatch({ type: 'SET_ERROR', payload: String(error) });
    }
  }, []);
  
  // 输入变化时更新补全候选
  const handleInputChange = useCallback((value: string) => {
    dispatch({ type: 'SET_INPUT_VALUE', payload: value });
    if (value.startsWith('/')) {
      const prefix = value.toLowerCase();
      const matches = COMMAND_REGISTRY
        .map(c => c.cmd)
        .filter(cmd => cmd.startsWith(prefix));
      dispatch({ type: 'SET_SUGGESTIONS', payload: matches });
    } else {
      dispatch({ type: 'SHOW_SUGGESTIONS', payload: false });
    }
  }, []);
  
  // 搜索查询变化时过滤消息
  React.useEffect(() => {
    if (!state.searchQuery) return;
    if (state.searchQuery === '') {
      dispatch({ type: 'SET_SEARCH_RESULTS', payload: { results: [], selected: 0 } });
      return;
    }
    const query = state.searchQuery.toLowerCase();
    const indices: number[] = [];
    state.messages.forEach((msg, i) => {
      const text = msg.content
        .filter(c => c.type === 'text')
        .map(c => c.text || '')
        .join(' ');
      if (text.toLowerCase().includes(query)) indices.push(i);
    });
    dispatch({ type: 'SET_SEARCH_RESULTS', payload: { results: indices, selected: 0 } });
  }, [state.searchQuery]);
  
  // 计算布局高度
  const messageHeight = height 
    - layout.statusBarHeight 
    - layout.inputHeight 
    - 4 // 边距和流式输出区
    - (state.isStreaming ? 3 : 0);
  
  return (
    <Box 
      flexDirection="column" 
      width={width}
      height={height}
      paddingX={layout.paddingX}
    >
      {/* 状态栏 */}
      <StatusBar 
        session={state.session}
        connected={state.connected}
        connecting={state.connecting}
        contextUsage={state.session?.contextUsage || 0}
        version={state.version}
      />
      
      {/* 搜索覆盖层 */}
      {state.searchQuery !== '' ? (
        <Box flexGrow={1} justifyContent="center" alignItems="flex-start" paddingTop={1}>
          <SearchOverlay
            query={state.searchQuery}
            messages={state.messages}
            results={state.searchResults}
            selectedIndex={state.selectedSearchResult}
            onQueryChange={(q) => dispatch({ type: 'SET_SEARCH_QUERY', payload: q })}
            onSelectChange={(i) => dispatch({ type: 'SET_SELECTED_SEARCH_RESULT', payload: i })}
            onClose={() => dispatch({ type: 'SET_SEARCH_QUERY', payload: '' })}
            onJump={() => dispatch({ type: 'SET_SEARCH_QUERY', payload: '' })}
          />
        </Box>
      ) : (
        <>
          {/* 消息列表 */}
          <Box 
            flexDirection="column"
            height={Math.max(messageHeight, 5)}
            flexGrow={1}
          >
            <MessageList 
              messages={state.messages}
              width={width - layout.paddingX * 2}
            />
          </Box>
          
          {/* 模型选择器 */}
          {state.showModelSelector && (
            <ModelSelector
              models={state.modelList}
              current={state.currentModel}
              onSelect={(model) => {
                ipcRef.current?.request('setModel', { model }).catch(() => {});
                dispatch({ type: 'SET_CURRENT_MODEL', payload: model });
              }}
              onClose={() => dispatch({ type: 'SHOW_MODEL_SELECTOR', payload: false })}
            />
          )}
          
          {/* 流式输出 */}
          {state.isStreaming && (
            <StreamOutput
              content={state.streamingContent}
              thinking={state.thinkingContent}
              tools={state.currentTools}
              width={width - layout.paddingX * 2}
            />
          )}
          
          {/* 输入区 */}
          <InputArea
            value={state.inputValue}
            mode={state.inputMode}
            focused={state.focused === 'input'}
            error={state.error}
            suggestions={state.suggestions}
            selectedSuggestion={state.selectedSuggestion}
            showSuggestions={state.showSuggestions}
            onChange={handleInputChange}
            onSubmit={sendMessage}
            onSelectSuggestion={(i) => {
              const cmd = state.suggestions[i];
              dispatch({ type: 'SET_INPUT_VALUE', payload: cmd + ' ' });
              dispatch({ type: 'SHOW_SUGGESTIONS', payload: false });
            }}
            onCloseSuggestions={() => dispatch({ type: 'SHOW_SUGGESTIONS', payload: false })}
            onClearError={() => dispatch({ type: 'CLEAR_ERROR' })}
          />
          
          {/* 快捷键栏 */}
          <ShortcutBar />
        </>
      )}
      
      {/* 帮助覆盖层 */}
      {state.showHelp && (
        <HelpOverlay width={width} height={height} />
      )}
      
      {/* 工具面板 */}
      {state.showTools && (
        <ToolsPanel 
          ipc={ipcRef.current}
          onClose={() => dispatch({ type: 'TOGGLE_TOOLS' })}
        />
      )}
    </Box>
  );
}

// 处理 IPC 事件
function handleIpcEvent(event: Record<string, unknown>, dispatch: React.Dispatch<Action>) {
  // 事件可能是 { type: 'assistant_chunk', ... } 或嵌套在 { event: { type: ... } }
  const eventType = (event.type || (event.event as Record<string, unknown>)?.type) as string;
  
  switch (eventType) {
    case 'assistant_chunk':
    case 'content_block_delta': {
      // 兼容两种格式
      const content = (event.content || (event.delta as Record<string, unknown>)?.text) as string;
      if (content) dispatch({ type: 'APPEND_STREAM', payload: content });
      break;
    }
    
    case 'thinking_chunk':
    case 'thinking_delta': {
      const content = (event.content || (event.thinking as string)) as string;
      if (content) dispatch({ type: 'APPEND_THINKING', payload: content });
      break;
    }
    
    case 'tool_use':
    case 'content_block_start': {
      const toolName = (event.tool_name || (event.content_block as Record<string, unknown>)?.name) as string;
      const toolId = (event.tool_use_id || (event.content_block as Record<string, unknown>)?.id) as string;
      const input = (event.input || (event.content_block as Record<string, unknown>)?.input) as Record<string, unknown>;
      
      if (toolName) {
        dispatch({
          type: 'SET_TOOL_STATE',
          payload: {
            id: toolId || `tool-${Date.now()}`,
            state: {
              name: toolName,
              status: 'running',
              input,
            },
          },
        });
      }
      break;
    }
    
    case 'tool_result':
    case 'content_block_stop': {
      const toolId = (event.tool_use_id || event.id) as string;
      if (toolId) {
        dispatch({
          type: 'SET_TOOL_STATE',
          payload: {
            id: toolId,
            state: {
              status: (event.is_error ? 'error' : 'success') as 'error' | 'success',
              output: event.output,
            },
          },
        });
      }
      break;
    }
    
    case 'result':
    case 'message_stop': {
      // 流式输出完成 - 不需要手动添加消息，内容已经通过 APPEND_STREAM 累积
      dispatch({ type: 'CLEAR_STREAM' });
      break;
    }
    
    case 'message_start': {
      // 新消息开始 - 设置流式状态
      dispatch({ type: 'SET_STREAMING', payload: true });
      break;
    }
    
    case 'error': {
      dispatch({ type: 'SET_ERROR', payload: (event.message || event.error) as string });
      break;
    }
    
    default:
      // 未知事件类型，打印调试
      if (process.env.DEBUG) {
        console.error('Unknown event type:', eventType, event);
      }
  }
}
