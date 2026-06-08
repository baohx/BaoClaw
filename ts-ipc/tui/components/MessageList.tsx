/**
 * 消息列表组件
 * 
 * 禅意设计：留白充足，层次分明，折叠长内容
 */
import React, { useState, useMemo } from 'react';
import { Box, Text, useInput } from 'ink';
import { colors, zen, toolIcons } from '../theme.js';
import type { Message, ContentBlock } from '../types.js';
import { renderMarkdown } from '../markdown.js';

interface MessageListProps {
  messages: Message[];
  width: number;
}

export function MessageList({ messages, width }: MessageListProps) {
  // 滚动状态
  const [scrollOffset, setScrollOffset] = useState(0);
  const visibleCount = 10;
  
  // 滚动到最新
  useMemo(() => {
    if (messages.length > visibleCount) {
      setScrollOffset(messages.length - visibleCount);
    }
  }, [messages.length]);
  
  // 键盘滚动
  useInput((input, key) => {
    if (key.upArrow) {
      setScrollOffset(Math.max(0, scrollOffset - 1));
    } else if (key.downArrow) {
      setScrollOffset(Math.min(messages.length - visibleCount, scrollOffset + 1));
    }
  });
  
  const visibleMessages = messages.slice(scrollOffset, scrollOffset + visibleCount);
  
  return (
    <Box flexDirection="column" gap={1}>
      {visibleMessages.map((message, index) => (
        <MessageItem 
          key={message.id} 
          message={message}
          width={width}
          isLast={index === visibleMessages.length - 1}
        />
      ))}
      
      {messages.length === 0 && (
        <Box paddingY={2}>
          <Text color="white">
            {zen.zenLine}
          </Text>
          <Text color="gray">
            {'  '}开始对话，按 ? 查看帮助
          </Text>
        </Box>
      )}
    </Box>
  );
}

interface MessageItemProps {
  message: Message;
  width: number;
  isLast: boolean;
}

function MessageItem({ message, width, isLast }: MessageItemProps) {
  const [expanded, setExpanded] = useState(false);
  
  return (
    <Box flexDirection="column">
      {/* 消息头部 */}
      <Box flexDirection="row" alignItems="center" gap={1}>
        {message.role === 'user' ? (
          <Text color="yellow" bold>❯ You</Text>
        ) : (
          <Text color="cyan" bold>◆ BaoClaw</Text>
        )}
        
        {/* 统计信息 */}
        {message.tokens && (
          <Text color="gray">
            {message.tokens.input + message.tokens.output}t
          </Text>
        )}
        {message.cost && (
          <Text color="yellow">
            ${message.cost.toFixed(4)}
          </Text>
        )}
        {message.duration && (
          <Text color="gray">
            {(message.duration / 1000).toFixed(1)}s
          </Text>
        )}
      </Box>
      
      {/* 消息内容 */}
      <Box paddingLeft={2} flexDirection="column">
        {message.content.map((block, index) => (
          <ContentBlockView 
            key={index} 
            block={block}
            width={width - 4}
            expanded={expanded}
            onToggleExpand={() => setExpanded(!expanded)}
          />
        ))}
      </Box>
      
      {/* 消息分隔 */}
      {!isLast && (
        <Box paddingTop={1}>
          <Text color="gray" dimColor>───── · ─────</Text>
        </Box>
      )}
    </Box>
  );
}

interface ContentBlockViewProps {
  block: ContentBlock;
  width: number;
  expanded: boolean;
  onToggleExpand: () => void;
}

function ContentBlockView({ block, width, expanded, onToggleExpand }: ContentBlockViewProps) {
  switch (block.type) {
    case 'text':
      return <TextView text={block.text || ''} width={width} />;
    case 'thinking':
      return <ThinkingView text={block.text || ''} width={width} />;
    case 'tool_use':
      return <ToolUseView name={block.toolName || ''} input={block.input} width={width} />;
    case 'tool_result':
      return <ToolResultView output={block.output} isError={block.isError} width={width} />;
    default:
      return null;
  }
}

interface TextViewProps {
  text: string;
  width: number;
}

function TextView({ text, width }: TextViewProps) {
  const elements = renderMarkdown(text);
  return (
    <Box flexDirection="column">
      {elements.map((el, i) => (
        <React.Fragment key={i}>{el}</React.Fragment>
      ))}
    </Box>
  );
}

interface ThinkingViewProps {
  text: string;
  width: number;
}

function ThinkingView({ text, width }: ThinkingViewProps) {
  const [show] = useState(false);
  
  if (!show) {
    return (
      <Text color="magenta" dimColor>
        ○ thinking...
      </Text>
    );
  }
  
  const lines = text.split('\n').slice(0, 10);
  
  return (
    <Box flexDirection="column">
      <Text color="magenta">💭 Thinking</Text>
      {lines.map((line, i) => (
        <Text key={i} color="gray">
          {line.slice(0, width - 4)}
        </Text>
      ))}
    </Box>
  );
}

interface ToolUseViewProps {
  name: string;
  input: Record<string, unknown> | undefined;
  width: number;
}

function ToolUseView({ name, input, width }: ToolUseViewProps) {
  const icon = toolIcons[name] || toolIcons.default;
  const inputPreview = formatInputPreview(name, input, width - 20);
  
  return (
    <Box flexDirection="row" alignItems="center" gap={1}>
      <Text color="magenta">
        {icon} {name}
      </Text>
      {inputPreview && (
        <Text color="gray">
          {inputPreview}
        </Text>
      )}
    </Box>
  );
}

interface ToolResultViewProps {
  output: unknown;
  isError: boolean | undefined;
  width: number;
}

function ToolResultView({ output, isError, width }: ToolResultViewProps) {
  const icon = isError ? '✗' : '✓';
  const color = isError ? 'red' : 'green';
  
  const outputStr = typeof output === 'string' 
    ? output 
    : JSON.stringify(output, null, 2);
  
  const preview = outputStr.split('\n').slice(0, 2).join('\n').slice(0, width - 10);
  
  return (
    <Box paddingLeft={2}>
      <Text color={color}>
        {icon} {preview}
        {outputStr.length > width - 10 && '...'}
      </Text>
    </Box>
  );
}

// 格式化输入预览
function formatInputPreview(toolName: string, input: Record<string, unknown> | undefined, maxWidth: number): string {
  if (!input) return '';
  
  switch (toolName) {
    case 'Bash':
      return String(input.command || '').slice(0, maxWidth);
    case 'FileRead':
    case 'FileWrite':
    case 'FileEdit':
      return String(input.file_path || '').slice(0, maxWidth);
    case 'Grep':
      return String(input.pattern || '').slice(0, maxWidth);
    default:
      const keys = Object.keys(input).slice(0, 2);
      return keys.map(k => `${k}=${String(input[k]).slice(0, 20)}`).join(' ');
  }
}
