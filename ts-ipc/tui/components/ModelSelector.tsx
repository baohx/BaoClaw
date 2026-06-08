/**
 * 模型切换弹层
 */
import React from 'react';
import { Box, Text } from 'ink';

interface ModelSelectorProps {
  models: string[];
  current: string;
  onSelect: (model: string) => void;
  onClose: () => void;
}

export function ModelSelector({ models, current, onSelect, onClose }: ModelSelectorProps) {
  return (
    <Box flexDirection="column" borderStyle="round" borderColor="cyan" paddingX={2} paddingY={1}>
      <Text color="cyan" bold>选择模型</Text>
      <Box flexDirection="column" marginTop={1}>
        {models.map((m) => {
          const isActive = m === current;
          return (
            <Text key={m} color={isActive ? 'green' : 'white'}>
              {isActive ? '⬤' : '○'} {m}
              {isActive ? '  ← 当前' : ''}
            </Text>
          );
        })}
      </Box>
      <Box marginTop={1}>
        <Text color="gray">↑↓ 选择  Enter 确认  Esc 关闭</Text>
      </Box>
    </Box>
  );
}
