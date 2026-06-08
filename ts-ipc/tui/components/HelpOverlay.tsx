/**
 * 帮助覆盖层
 */
import React from 'react';
import { Box, Text } from 'ink';

interface HelpOverlayProps {
  width: number;
  height: number;
}

export function HelpOverlay({ width, height }: HelpOverlayProps) {
  const shortcuts = [
    { key: 'Enter', action: 'Send message' },
    { key: '↑/↓', action: 'Scroll / Browse' },
    { key: '←/→', action: 'Move cursor' },
    { key: 'Ctrl+C', action: 'Exit' },
    { key: 'Ctrl+R', action: 'Search messages' },
    { key: '?', action: 'Toggle help' },
    { key: 'Tab', action: 'Complete' },
    { key: 't', action: 'Tools panel' },
  ];
  
  const commands = [
    { cmd: '/help', desc: 'Show help' },
    { cmd: '/status', desc: 'Session status' },
    { cmd: '/model [name]', desc: 'Switch model' },
    { cmd: '/clear', desc: 'Clear chat' },
    { cmd: '/compact', desc: 'Compact context' },
    { cmd: '/sessions', desc: 'List sessions' },
    { cmd: '/tools', desc: 'View tools' },
    { cmd: '/memory', desc: 'Memory mgmt' },
    { cmd: '/cron', desc: 'Cron jobs' },
    { cmd: '/git', desc: 'Git status' },
    { cmd: '/search <q>', desc: 'Search messages' },
    { cmd: '/skills', desc: 'Active skills' },
    { cmd: '/gateway', desc: 'Gateway status' },
  ];
  
  const maxWidth = Math.min(width - 4, 60);
  
  return (
    <Box 
      flexDirection="column"
      alignItems="center"
      justifyContent="center"
      width={width}
      height={height}
    >
      <Box 
        flexDirection="column"
        borderStyle="round"
        borderColor="yellow"
        paddingX={2}
        paddingY={1}
        width={maxWidth}
      >
        {/* 标题 */}
        <Box marginBottom={1}>
          <Text color="yellow" bold>
            ● BaoClaw Help
          </Text>
        </Box>
        
        {/* 快捷键 */}
        <Box flexDirection="column" marginBottom={1}>
          <Text color="white" bold>
            Shortcuts
          </Text>
          {shortcuts.map(({ key, action }) => (
            <Box key={key} flexDirection="row">
              <Text color="cyan">{key.padEnd(12)}</Text>
              <Text color="gray">{action}</Text>
            </Box>
          ))}
        </Box>
        
        {/* 命令 */}
        <Box flexDirection="column" marginTop={1}>
          <Text color="white" bold>
            Commands
          </Text>
          {commands.map(({ cmd, desc }) => (
            <Box key={cmd} flexDirection="row">
              <Text color="yellow">{cmd.padEnd(12)}</Text>
              <Text color="gray">{desc}</Text>
            </Box>
          ))}
        </Box>
        
        {/* 底部 */}
        <Box marginTop={1}>
          <Text color="gray">Press ESC or ? to close</Text>
        </Box>
      </Box>
    </Box>
  );
}
