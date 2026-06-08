/**
 * 底部常驻快捷键栏
 *
 * Enter:Send  ↑↓:History  Ctrl+R:Search  ?:Help  Esc:Close
 */
import React from 'react';
import { Box, Text } from 'ink';

const shortcuts = [
  { key: 'Enter', action: 'Send' },
  { key: '↑↓', action: 'History' },
  { key: 'Ctrl+R', action: 'Search' },
  { key: '?', action: 'Help' },
  { key: 'Esc', action: 'Close' },
];

export function ShortcutBar() {
  return (
    <Box flexDirection="row" justifyContent="center" gap={2} paddingX={1}>
      {shortcuts.map((s, i) => (
        <Box key={s.key} flexDirection="row" gap={0}>
          <Text color="cyan" bold>{s.key}</Text>
          <Text color="gray">:{s.action}</Text>
          {i < shortcuts.length - 1 && <Text color="gray">  </Text>}
        </Box>
      ))}
    </Box>
  );
}
