import React from 'react';
import { Text, Box } from 'ink';
import { colors, zen } from '../theme.js';
import { ToolProgress } from '../types.js';

interface ToolsPanelProps {
  tools: ToolProgress[];
}

export const ToolsPanel: React.FC<ToolsPanelProps> = ({ tools }) => {
  if (tools.length === 0) return null;

  return (
    <Box 
      flexDirection="column"
      borderStyle="round"
      borderColor={colors.tool}
      paddingX={1}
      marginY={1}
    >
      <Box marginBottom={1}>
        <Text color={colors.tool} bold>
          Tools
        </Text>
      </Box>

      {tools.map((tool, idx) => {
        const statusIcon = tool.status === 'running' 
          ? '◐' 
          : tool.status === 'error' 
            ? zen.cross 
            : zen.check;

        const statusColor = tool.status === 'running' 
          ? colors.status.warning 
          : tool.status === 'error' 
            ? colors.status.error 
            : colors.status.success;

        return (
          <Box key={idx}>
            <Text color={statusColor}>
              {statusIcon}
            </Text>
            <Text color={colors.text.primary}>
              {' '}{tool.name}
            </Text>
            {tool.output && (
              <Text color={colors.text.dim}>
                {' '}{zen.arrow} {tool.output.slice(0, 50)}
                {tool.output.length > 50 ? '...' : ''}
              </Text>
            )}
          </Box>
        );
      })}
    </Box>
  );
};

export default ToolsPanel;
