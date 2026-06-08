/**
 * 流式输出组件
 * 
 * 实时显示：思考过程、工具执行、流式文本
 */
import React from 'react';
import { Box, Text } from 'ink';
import { colors, zen, toolIcons, timing } from '../theme.js';
import type { ToolState } from '../types.js';

interface StreamOutputProps {
  content: string;
  thinking: string;
  tools: Map<string, ToolState>;
  width: number;
}

export function StreamOutput({ content, thinking, tools, width }: StreamOutputProps) {
  const hasThinking = thinking.length > 0;
  const hasContent = content.length > 0;
  const hasTools = tools.size > 0;
  
  if (!hasThinking && !hasContent && !hasTools) return null;
  
  return (
    <Box flexDirection="column" paddingX={1}>
      {/* ── 思考过程 ── */}
      {hasThinking && (
        <Box flexDirection="column">
          <Text color="magenta" bold>💭 思考中</Text>
          <ThinkingProgress />
        </Box>
      )}
      
      {/* ── 工具执行 ── */}
      {hasTools && (
        <Box flexDirection="column" paddingY={1}>
          <Text color="yellow" bold>🔧 工具执行中</Text>
          {Array.from(tools.entries()).map(([id, tool]) => (
            <ToolProgressBar key={id} tool={tool} width={width - 6} />
          ))}
        </Box>
      )}
      
      {/* ── 回复输出 ── */}
      {hasContent && (
        <Box flexDirection="column" paddingY={1}>
          <Text color="cyan" bold>✦ 回复中</Text>
          <Box paddingLeft={2}>
            <Text color="white">
              {content.slice(-width)}
            </Text>
          </Box>
        </Box>
      )}
    </Box>
  );
}

interface ToolProgressBarProps {
  tool: ToolState;
  width: number;
}

function ToolProgressBar({ tool, width }: ToolProgressBarProps) {
  const icon = toolIcons[tool.name] || zen.dot;
  const inputPreview = formatToolInput(tool.name, tool.input, width - 20);
  
  return (
    <Box flexDirection="row" alignItems="center" gap={1}>
      {/* 状态图标 */}
      {tool.status === 'running' ? (
        <Spinner color="magenta" />
      ) : tool.status === 'success' ? (
        <Text color="green">●</Text>
      ) : (
        <Text color="red">●</Text>
      )}
      
      {/* 工具名称 */}
      <Text color="magenta">
        {icon} {tool.name}
      </Text>
      
      {/* 输入预览 */}
      {inputPreview && (
        <Text color="gray">
          {inputPreview}
        </Text>
      )}
      
      {/* 执行时间 */}
      {tool.startTime && tool.status === 'running' && (
        <Text color="gray">
          {(Date.now() - tool.startTime) / 1000}s
        </Text>
      )}
    </Box>
  );
}

// 旋转动画组件
function Spinner({ color }: { color: string }) {
  const [frame, setFrame] = React.useState(0);
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
  
  React.useEffect(() => {
    const timer = setInterval(() => {
      setFrame((f) => (f + 1) % frames.length);
    }, 200);
    return () => clearInterval(timer);
  }, []);
  
  return <Text color={color}>{frames[frame]}</Text>;
}

// 格式化工具输入
function formatToolInput(name: string, input: Record<string, unknown> | undefined, maxWidth: number): string {
  if (!input) return '';
  
  switch (name) {
    case 'Bash':
      const cmd = String(input.command || '');
      return cmd.length > maxWidth ? cmd.slice(0, maxWidth) + '…' : cmd;
    case 'FileRead':
    case 'FileWrite':
    case 'FileEdit':
      return String(input.file_path || '');
    case 'Grep':
      return `/${input.pattern}/`;
    default:
      return '';
  }
}

function ThinkingProgress() {
  const [elapsed, setElapsed] = React.useState(0);
  const [frame, setFrame] = React.useState(0);
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

  React.useEffect(() => {
    const timer = setInterval(() => {
      setElapsed((e) => e + 0.5);
      setFrame((f) => (f + 1) % frames.length);
    }, 500);
    return () => clearInterval(timer);
  }, []);

  const progress = Math.min(elapsed / 10 * 100, 95);
  const filled = Math.floor(progress / 10);
  const empty = 10 - filled;

  return (
    <Box flexDirection="row" alignItems="center" gap={1}>
      <Text color="magenta">{frames[frame]}</Text>
      <Text color="magenta">thinking...</Text>
      <Text color="gray">({elapsed.toFixed(1)}s)</Text>
      <Text>
        <Text color="green">{'█'.repeat(filled)}</Text>
        <Text color="gray">{'░'.repeat(empty)}</Text>
      </Text>
      <Text color="gray">{progress.toFixed(0)}%</Text>
    </Box>
  );
}
