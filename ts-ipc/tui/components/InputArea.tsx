/**
 * 输入区域组件
 * 
 * 极简输入框：❯ _
 */
import React, { useState, useCallback } from 'react';
import { Box, Text, useInput } from 'ink';
import { colors, zen } from '../theme.js';
import { COMMAND_REGISTRY } from '../state.js';
import type { TuiState } from '../types.js';

interface InputAreaProps {
  value: string;
  mode: 'normal' | 'multiline' | 'command';
  focused: boolean;
  error: string | null;
  onChange: (value: string) => void;
  onSubmit: (value: string) => void;
  onClearError: () => void;
  suggestions: string[];
  selectedSuggestion: number;
  showSuggestions: boolean;
  onSelectSuggestion: (index: number) => void;
  onCloseSuggestions: () => void;
}

export function InputArea({ 
  value, 
  mode, 
  focused, 
  error,
  onChange, 
  onSubmit,
  onClearError,
  suggestions,
  selectedSuggestion,
  showSuggestions,
  onSelectSuggestion,
  onCloseSuggestions
}: InputAreaProps) {
  
  // 键盘输入 — 简化为行末追加模式，避免 Unicode/IME 光标错乱
  useInput(useCallback((input, key) => {
    if (!focused) return;
    
    if (error) onClearError();
    
    // 回车提交
    if (key.return) {
      if (mode === 'multiline' && !key.shift) {
        onChange(value + '\n');
      } else {
        onSubmit(value);
      }
      return;
    }
    
    // 退格 — 删最后一个字符
    if (key.backspace || key.delete) {
      if (value.length > 0) {
        onChange(value.slice(0, -1));
      }
      return;
    }
    
    // Tab 补全
    if (key.tab && focused && showSuggestions) {
      onSelectSuggestion(selectedSuggestion);
      return;
    }
    // ↑↓ 在补全候选间移动
    if (key.upArrow && focused && showSuggestions) {
      onSelectSuggestion(Math.max(0, selectedSuggestion - 1));
      return;
    }
    if (key.downArrow && focused && showSuggestions) {
      onSelectSuggestion(Math.min(suggestions.length - 1, selectedSuggestion + 1));
      return;
    }
    // Esc 关闭候选
    if (key.escape && focused && showSuggestions) {
      onCloseSuggestions();
      return;
    }
    
    // 普通字符输入 — 始终追加到末尾
    if (input && !key.ctrl && !key.meta) {
      onChange(value + input);
    }
  }, [value, mode, focused, error, showSuggestions, selectedSuggestion, suggestions.length, onChange, onSubmit, onClearError, onSelectSuggestion, onCloseSuggestions]));
  
  const lines = value.split('\n');
  const displayLines = Math.min(lines.length, 3);
  
  return (
    <Box flexDirection="column">
      {/* 错误提示 */}
      {error && (
        <Box marginBottom={1}>
          <Text color="red" bold>
            ● {error}
          </Text>
        </Box>
      )}
      
      {/* 自动补全弹窗 */}
      {showSuggestions && suggestions.length > 0 && (
        <Box flexDirection="column" marginBottom={1} paddingLeft={2}>
          <Box borderStyle="single" borderColor="cyan" flexDirection="column" paddingX={1}>
            {suggestions.map((s, i) => (
              <Text key={s} color={i === selectedSuggestion ? 'cyan' : 'gray'} bold={i === selectedSuggestion}>
                {i === selectedSuggestion ? '❯ ' : '  '}{s}
              </Text>
            ))}
          </Box>
        </Box>
      )}
      
      {/* 输入框 */}
      <Box 
        flexDirection="row"
        alignItems="flex-start"
      >
        {/* 提示符 */}
        <Text color="yellow" bold>
          ❯ 
        </Text>
        
        {/* 输入内容 */}
        <Box flexDirection="column" flexGrow={1}>
          {lines.slice(0, displayLines).map((line, i) => (
            <Text key={i} color="white">
              {line}
              {i === lines.length - 1 && focused && <Cursor />}
            </Text>
          ))}
          
          {lines.length > displayLines && (
            <Text color="gray">
              ... ({lines.length - displayLines} more lines)
            </Text>
          )}
        </Box>
      </Box>
      
      {/* 模式提示 */}
      {mode !== 'normal' && (
        <Text color="gray">
          {mode === 'multiline' ? 'Shift+Enter = new line, Enter = send' : 'Command mode'}
        </Text>
      )}
    </Box>
  );
}

// 光标组件
function Cursor() {
  const [visible, setVisible] = useState(true);
  
  React.useEffect(() => {
    const timer = setInterval(() => setVisible((v) => !v), 500);
    return () => clearInterval(timer);
  }, []);
  
  return (
    <Text backgroundColor="white" color="black">
      {visible ? ' ' : ''}
    </Text>
  );
}
