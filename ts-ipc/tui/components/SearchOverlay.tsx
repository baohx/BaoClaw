/**
 * Ctrl+R 消息搜索覆盖层
 */
import React, { useCallback } from 'react';
import { Box, Text, useInput } from 'ink';
import type { Message } from '../types.js';

interface SearchOverlayProps {
  query: string;
  messages: Message[];
  results: number[];
  selectedIndex: number;
  onQueryChange: (q: string) => void;
  onSelectChange: (i: number) => void;
  onClose: () => void;
  onJump: (msgIndex: number) => void;
}

export function SearchOverlay({
  query,
  messages,
  results,
  selectedIndex,
  onQueryChange,
  onSelectChange,
  onClose,
  onJump,
}: SearchOverlayProps) {
  useInput(
    useCallback(
      (input, key) => {
        if (key.escape) { onClose(); return; }
        if (key.return && results.length > 0) { onJump(results[selectedIndex]); return; }
        if (key.upArrow && results.length > 0) { onSelectChange(Math.max(0, selectedIndex - 1)); return; }
        if (key.downArrow && results.length > 0) { onSelectChange(Math.min(results.length - 1, selectedIndex + 1)); return; }
        if (input && !key.ctrl && !key.meta) { onQueryChange(query + input); return; }
        if (key.backspace || key.delete) { onQueryChange(query.slice(0, -1)); return; }
      },
      [query, results, selectedIndex, onQueryChange, onSelectChange, onClose, onJump],
    ),
  );

  return (
    <Box flexDirection="column" borderStyle="round" borderColor="yellow" paddingX={2} paddingY={1}>
      <Text color="yellow" bold>Ctrl+R 搜索消息</Text>
      <Box marginY={1}><Text color="cyan">❯ </Text><Text color="white">{query}</Text><Text color="gray">_</Text></Box>
      <Box flexDirection="column" marginY={1}>
        {results.length === 0 ? (
          <Text color="gray">无匹配结果</Text>
        ) : (
          results.slice(0, 10).map((msgIdx, i) => {
            const msg = messages[msgIdx];
            const preview = getPreview(msg, query);
            const isSelected = i === selectedIndex;
            return (
              <Box key={msgIdx} flexDirection="row">
                <Text color={isSelected ? 'cyan' : 'gray'}>
                  {isSelected ? '❯ ' : '  '}[{msgIdx + 1}] {msg.role === 'user' ? 'You' : '✦'}
                </Text>
                <Text color="white"> {preview}</Text>
              </Box>
            );
          })
        )}
      </Box>
      <Text color="gray">↑↓ 浏览  Enter 跳转  Esc 关闭  ({selectedIndex + 1}/{results.length})</Text>
    </Box>
  );
}

function getPreview(msg: Message, query: string): string {
  const text = msg.content.filter((c) => c.type === 'text').map((c) => c.text || '').join(' ');
  const idx = text.toLowerCase().indexOf(query.toLowerCase());
  if (idx === -1) return text.slice(0, 60);
  const start = Math.max(0, idx - 20);
  const end = Math.min(text.length, idx + query.length + 40);
  return (start > 0 ? '…' : '') + text.slice(start, end) + (end < text.length ? '…' : '');
}
