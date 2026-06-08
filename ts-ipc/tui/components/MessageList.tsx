/**
 * 消息列表 — 清晰分层：Q&A 分离、输出类型标签、滚动条
 */
import React, { useState } from 'react';
import { Box, Text, useInput } from 'ink';
import { colors, zen, toolIcons } from '../theme.js';
import type { Message, ContentBlock } from '../types.js';
import { renderMarkdown } from '../markdown.js';

interface MessageListProps {
  messages: Message[];
  width: number;
  maxHeight?: number;
}

export function MessageList({ messages, width, maxHeight = 40 }: MessageListProps) {
  const visibleCount = Math.max(3, Math.floor(maxHeight / 4));
  const [scrollOffset, setScrollOffset] = useState(0);
  
  React.useEffect(() => {
    if (messages.length > visibleCount) {
      setScrollOffset(messages.length - visibleCount);
    }
  }, [messages.length, visibleCount]);
  
  useInput((input, key) => {
    if (key.upArrow) {
      setScrollOffset(Math.max(0, scrollOffset - 1));
    } else if (key.downArrow) {
      setScrollOffset(Math.min(messages.length - visibleCount, scrollOffset + 1));
    }
  });
  
  const visibleMessages = messages.slice(scrollOffset, scrollOffset + visibleCount);
  const hasMoreAbove = scrollOffset > 0;
  const hasMoreBelow = scrollOffset + visibleCount < messages.length;
  
  return (
    <Box flexDirection="column">
      {/* ── 滚动条 ── */}
      {messages.length > 0 && (
        <Box flexDirection="row" justifyContent="center" paddingBottom={1}>
          <Text color="gray" dimColor>
            {hasMoreAbove ? '▲' : ' '}  [{scrollOffset + 1}–{scrollOffset + visibleMessages.length} / {messages.length}]  {hasMoreBelow ? '▼' : ' '}
          </Text>
        </Box>
      )}
      
      {/* ── 空状态 ── */}
      {messages.length === 0 && (
        <Box paddingY={2}>
          <Text color="white">{zen.zenLine}</Text>
          <Text color="gray">{'  '}开始对话，按 ? 查看帮助</Text>
        </Box>
      )}
      
      {/* ── 消息列表 ── */}
      {visibleMessages.map((message) => (
        <MessageItem 
          key={message.id} 
          message={message}
          width={width - 2}
        />
      ))}
    </Box>
  );
}

// ═══════════════════════════════════════════════════════
// 单条消息 — 用户/助手清晰分层
// ═══════════════════════════════════════════════════════

function MessageItem({ message, width }: { message: Message; width: number }) {
  const isUser = message.role === 'user';
  const borderColor = isUser ? 'yellow' : 'cyan';
  const label = isUser ? '❯ You' : '◆ BaoClaw';
  const labelColor = isUser ? 'yellow' : 'cyan';
  
  return (
    <Box flexDirection="column" marginBottom={1}>
      {/* ── Q&A 分隔条 ── */}
      <Box flexDirection="row">
        <Text color={borderColor}>
          {isUser
            ? '┌' + '─'.repeat(Math.min(width - 1, 60))
            : '┌' + '─'.repeat(Math.min(width - 1, 60))}
        </Text>
      </Box>
      
      {/* ── 角色标签 ── */}
      <Box flexDirection="row" paddingLeft={1}>
        <Text color={borderColor}>│ </Text>
        <Text color={labelColor} bold>{label}</Text>
        {message.tokens && (
          <Text color="gray">  {message.tokens.input + message.tokens.output}t</Text>
        )}
        {message.cost && (
          <Text color="yellow">  ${message.cost.toFixed(4)}</Text>
        )}
        {message.duration && (
          <Text color="gray">  {(message.duration / 1000).toFixed(1)}s</Text>
        )}
      </Box>
      
      {/* ── 消息内容 ── */}
      <Box flexDirection="column" paddingLeft={2}>
        {message.content.map((block, i) => (
          <ContentBlockView 
            key={i} 
            block={block}
            width={width - 4}
            blockIndex={i}
          />
        ))}
        
        {/* 用户消息留空行，助手消息输出完毕标记 */}
        {!isUser && message.content.length > 0 && (
          <Box paddingTop={1}>
            <Text color="gray" dimColor>└ 回复完毕</Text>
          </Box>
        )}
      </Box>
    </Box>
  );
}

// ═══════════════════════════════════════════════════════
// 内容块 — 清晰标签：💭思考 / 🔧工具 / 📤输出 / ✦结果
// ═══════════════════════════════════════════════════════

interface ContentBlockViewProps {
  block: ContentBlock;
  width: number;
  blockIndex: number;
}

function ContentBlockView({ block, width }: ContentBlockViewProps) {
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

// ── 文本输出 ──
function TextView({ text, width }: { text: string; width: number }) {
  const elements = renderMarkdown(text);
  return (
    <Box flexDirection="column" paddingY={1}>
      <Text color="gray" dimColor>✦ 回复</Text>
      <Box flexDirection="column" paddingLeft={2}>
        {elements.map((el, i) => (
          <React.Fragment key={i}>{el}</React.Fragment>
        ))}
      </Box>
    </Box>
  );
}

// ── 思考过程 ──
function ThinkingView({ text, width }: { text: string; width: number }) {
  const [expanded, setExpanded] = useState(false);
  const lines = text.split('\n');
  const preview = lines.slice(0, expanded ? 20 : 1);
  
  return (
    <Box flexDirection="column" paddingY={1}>
      <Box flexDirection="row" gap={1}>
        <Text color="magenta">💭 思考过程</Text>
        {lines.length > 1 && (
          <Text color="gray" dimColor>
            ({lines.length} 行 — 按 t 展开)
          </Text>
        )}
      </Box>
      <Box flexDirection="column" paddingLeft={2}>
        {preview.map((line, i) => (
          <Text key={i} color="magenta" dimColor>
            {line.slice(0, width - 4)}
            {line.length > width - 4 && '…'}
          </Text>
        ))}
      </Box>
    </Box>
  );
}

// ── 工具调用 ──
function ToolUseView({ name, input, width }: { name: string; input: Record<string, unknown> | undefined; width: number }) {
  const icon = toolIcons[name] || '⚡';
  const preview = formatToolInput(name, input, width - 20);
  
  return (
    <Box flexDirection="column" paddingY={1}>
      <Box flexDirection="row" gap={1}>
        <Text color="yellow">🔧 工具调用</Text>
        <Text color="cyan" bold>
          {icon} {name}
        </Text>
      </Box>
      {preview && (
        <Box paddingLeft={2}>
          <Text color="gray">{preview}</Text>
        </Box>
      )}
    </Box>
  );
}

// ── 工具输出 ──
function ToolResultView({ output, isError, width }: { output: unknown; isError: boolean | undefined; width: number }) {
  const statusIcon = isError ? '✗' : '✓';
  const statusColor = isError ? 'red' : 'green';
  
  const outputStr = typeof output === 'string' 
    ? output 
    : JSON.stringify(output, null, 2);
  
  const lines = outputStr.split('\n');
  const preview = lines.slice(0, 3);
  
  return (
    <Box flexDirection="column" paddingY={1}>
      <Box flexDirection="row" gap={1}>
        <Text color="green">📤 工具输出</Text>
        <Text color={statusColor} bold>{statusIcon}</Text>
      </Box>
      <Box flexDirection="column" paddingLeft={2}>
        {preview.map((line, i) => (
          <Text key={i} color="gray">
            {line.slice(0, width - 4)}
            {line.length > width - 4 && '…'}
          </Text>
        ))}
        {lines.length > 3 && (
          <Text color="gray" dimColor>… 共 {lines.length} 行</Text>
        )}
      </Box>
    </Box>
  );
}

// ── 工具输入预览 ──
function formatToolInput(name: string, input: Record<string, unknown> | undefined, maxWidth: number): string {
  if (!input) return '';
  switch (name) {
    case 'Bash':
      return String(input.command || '').slice(0, maxWidth);
    case 'FileRead':
    case 'FileWrite':
    case 'FileEdit':
      return String(input.file_path || '').slice(0, maxWidth);
    case 'WebSearchTool':
      return String(input.query || '').slice(0, maxWidth);
    case 'WebFetchTool':
      return String(input.url || '').slice(0, maxWidth);
    default:
      const keys = Object.keys(input).slice(0, 2);
      return keys.map(k => `${k}=${String(input[k]).slice(0, 20)}`).join(' ');
  }
}
