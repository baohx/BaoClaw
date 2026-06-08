/**
 * 状态栏组件
 * 
 * 极简设计：一行显示关键状态
 * 
 * ● Session-abc123 · Claude Sonnet · ▓▓▓▓▓░░░░░ 45% · $0.0023
 */
import React from 'react';
import { Box, Text } from 'ink';
import { colors, zen } from '../theme.js';
import type { SessionState } from '../types.js';

interface StatusBarProps {
  session: SessionState | null;
  connected: boolean;
  connecting: boolean;
  contextUsage: number;
  version?: string;
}

export function StatusBar({ session, connected, connecting, contextUsage, version }: StatusBarProps) {
  // 上下文使用率进度条
  const progressBar = renderProgressBar(contextUsage);
  
  // 状态指示器 - 使用亮色
  const statusIndicator = connecting ? (
    <Text color="yellow">◐</Text>
  ) : connected ? (
    <Text color="green">{zen.dot}</Text>
  ) : (
    <Text color="red">{zen.empty}</Text>
  );
  
  // 格式化成本
  const formatCost = (cost: number) => {
    if (cost < 0.01) return `$${cost.toFixed(4)}`;
    return `$${cost.toFixed(2)}`;
  };
  
  // 格式化 token 数
  const formatTokens = (tokens: number) => {
    if (tokens >= 1000000) return `${(tokens / 1000000).toFixed(1)}M`;
    if (tokens >= 1000) return `${(tokens / 1000).toFixed(1)}K`;
    return String(tokens);
  };
  
  return (
    <Box 
      flexDirection="row"
      justifyContent="space-between"
      alignItems="center"
      paddingX={1}
    >
      {/* 左侧：连接状态 + 会话信息 */}
      <Box flexDirection="row" alignItems="center" gap={1}>
        {statusIndicator}
        {session && (
          <>
            <Text color="white" bold>
              {session.id ? session.id.slice(0, 8) : '--------'}
            </Text>
            <Text color="gray">{zen.separator}</Text>
            <Text color="cyan">{session.model || 'unknown'}</Text>
          </>
        )}
      </Box>
      
      {/* 右侧：资源使用 */}
      {session && (
        <Box flexDirection="row" alignItems="center" gap={1}>
          {/* 上下文使用 */}
          <Text>
            {progressBar}
            <Text color="white"> {contextUsage.toFixed(0)}%</Text>
          </Text>
          
          <Text color="gray">{zen.separator}</Text>
          
          {/* Token 数 */}
          <Text color="white">
            {formatTokens(session.totalTokens || 0)}
          </Text>
          
          <Text color="gray">{zen.separator}</Text>
          
          {/* 成本 */}
          <Text color="yellow">
            {formatCost(session.totalCost || 0)}
          </Text>
          <Text color="gray">{zen.separator}</Text>
          <Text color="gray" dimColor>v{version || '?'}</Text>
        </Box>
      )}
    </Box>
  );
}

// 渲染进度条
function renderProgressBar(percentage: number): React.ReactNode {
  const filled = Math.floor(percentage / 10);
  const empty = 10 - filled;
  
  return (
    <Text>
      <Text color="green">{'█'.repeat(filled)}</Text>
      <Text color="gray">{'░'.repeat(empty)}</Text>
    </Text>
  );
}
