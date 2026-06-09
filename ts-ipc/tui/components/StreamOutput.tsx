/**
 * 流式输出组件 — 固定高度三块区域，互不压制
 * 
 * 布局（总计 10 行）：
 *   💭 思考中   [5行固定，内部滚动]
 *   🔧 工具中   [3行固定，超 3 行折叠]
 *   ✦ 回复中   [2行，尾部文本]
 */
import React from 'react';
import { Box, Text } from 'ink';
import { colors, zen, toolIcons } from '../theme.js';
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
    <Box flexDirection="column" paddingX={1} paddingTop={1} flexShrink={0}>
      {/* ── 💭 思考中 — 固定 5 行 ── */}
      <ThinkingSection thinking={thinking} width={width} visible={hasThinking} />

      {/* ── 🔧 工具中 — 固定 3 行 ── */}
      <ToolSection tools={tools} width={width} visible={hasTools} />

      {/* ── ✦ 回复中 — 固定 2 行 ── */}
      <TextSection content={content} width={width} visible={hasContent} />
    </Box>
  );
}

// ═══════════════════════════════════════════════════════
// 💭 思考区 — 固定 5 行，内部滚动显示最后 5 行
// ═══════════════════════════════════════════════════════

function ThinkingSection({ thinking, width, visible }: { thinking: string; width: number; visible: boolean }) {
  if (!visible) return null;

  const lines = thinking.split('\n');
  // 只显示最后 5 行
  const visibleLines = lines.slice(-5);
  const totalLines = lines.length;
  const moreLines = totalLines > 5;

  return (
    <Box flexDirection="column" height={5} flexShrink={0}>
      <Box flexDirection="row" gap={1}>
        <Text color="magenta" bold>💭 思考中</Text>
        {moreLines && (
          <Text color="gray" dimColor>
            ({totalLines} 行，显示最后 5 行)
          </Text>
        )}
        <ThinkingTimer />
      </Box>
      <Box flexDirection="column" paddingLeft={2}>
        {visibleLines.map((line, i) => (
          <Text key={i} color="magenta" dimColor>
            {line.slice(0, width - 4)}
            {line.length > width - 4 ? '…' : ''}
          </Text>
        ))}
      </Box>
      {/* 填充空白行确保固定高度 */}
      {Array.from({ length: Math.max(0, 4 - visibleLines.length) }).map((_, i) => (
        <Text key={`pad-${i}`}> </Text>
      ))}
    </Box>
  );
}

// ── 思考计时器 ──
function ThinkingTimer() {
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

  return (
    <Text color="gray">
      {frames[frame]} {elapsed.toFixed(1)}s
    </Text>
  );
}

// ═══════════════════════════════════════════════════════
// 🔧 工具区 — 固定 3 行，超 3 个折叠
// ═══════════════════════════════════════════════════════

function ToolSection({ tools, width, visible }: { tools: Map<string, ToolState>; width: number; visible: boolean }) {
  if (!visible) return null;

  const toolList = Array.from(tools.entries());
  const displayTools = toolList.slice(-3); // 最多 3 个

  return (
    <Box flexDirection="column" height={3} flexShrink={0} paddingY={1}>
      <Box flexDirection="row" gap={1}>
        <Text color="yellow" bold>🔧 工具中</Text>
        {toolList.length > 3 && (
          <Text color="gray" dimColor>({toolList.length} 个，显示最后 3 个)</Text>
        )}
      </Box>
      <Box flexDirection="column" paddingLeft={2}>
        {displayTools.map(([id, tool]) => (
          <ToolLine key={id} tool={tool} width={width - 6} />
        ))}
        {Array.from({ length: Math.max(0, 2 - displayTools.length) }).map((_, i) => (
          <Text key={`pad-${i}`}> </Text>
        ))}
      </Box>
    </Box>
  );
}

function ToolLine({ tool, width }: { tool: ToolState; width: number }) {
  const icon = toolIcons[tool.name] || '⚡';
  const spinner = tool.status === 'running' ? (
    <ToolSpinner />
  ) : tool.status === 'success' ? (
    <Text color="green">●</Text>
  ) : (
    <Text color="red">●</Text>
  );

  const inputPreview = formatToolInput(tool.name, tool.input, width - 15);

  return (
    <Box flexDirection="row" gap={1}>
      {spinner}
      <Text color="yellow">{icon} {tool.name}</Text>
      {inputPreview && <Text color="gray">{inputPreview}</Text>}
    </Box>
  );
}

// ── 工具旋转指示器 ──
function ToolSpinner() {
  const [frame, setFrame] = React.useState(0);
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

  React.useEffect(() => {
    const timer = setInterval(() => setFrame((f) => (f + 1) % frames.length), 200);
    return () => clearInterval(timer);
  }, []);

  return <Text color="magenta">{frames[frame]}</Text>;
}

// ═══════════════════════════════════════════════════════
// ✦ 回复区 — 固定 2 行，显示尾部文本
// ═══════════════════════════════════════════════════════

function TextSection({ content, width, visible }: { content: string; width: number; visible: boolean }) {
  if (!visible) return null;

  const lines = content.split('\n');
  const last2 = lines.slice(-2);

  return (
    <Box flexDirection="column" height={2} flexShrink={0} paddingY={1}>
      <Text color="cyan" bold>✦ 回复中</Text>
      <Box paddingLeft={2}>
        {last2.map((line, i) => (
          <Text key={i} color="white">
            {line.slice(-width + 4)}
          </Text>
        ))}
      </Box>
    </Box>
  );
}

// ═══════════════════════════════════════════════════════
// 工具函数
// ═══════════════════════════════════════════════════════

function formatToolInput(name: string, input: Record<string, unknown> | undefined, maxWidth: number): string {
  if (!input) return '';
  switch (name) {
    case 'Bash':
      const cmd = String(input.command || '');
      return cmd.slice(0, maxWidth) + (cmd.length > maxWidth ? '…' : '');
    case 'FileRead':
    case 'FileWrite':
    case 'FileEdit':
      return String(input.file_path || '').slice(0, maxWidth);
    case 'WebSearchTool':
      return String(input.query || '').slice(0, maxWidth);
    case 'WebFetchTool':
      return String(input.url || '').slice(0, maxWidth);
    default:
      return '';
  }
}
