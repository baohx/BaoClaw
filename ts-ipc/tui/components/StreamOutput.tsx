import React from 'react';
import { Text, Box } from 'ink';
import { colors, zen } from '../theme.js';
import { ToolProgress } from '../types.js';

interface StreamOutputProps {
  content: string;
  thinking?: string;
  tools?: ToolProgress[];
}

const ToolIndicator: React.FC<{ tool: ToolProgress }> = ({ tool }) => {
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
    <Box marginX={1}>
      <Text color={statusColor}>
        {statusIcon} {tool.name}
      </Text>
    </Box>
  );
};

export const StreamOutput: React.FC<StreamOutputProps> = ({ 
  content, 
  thinking, 
  tools = [] 
}) => {
  return (
    <Box flexDirection="column" marginY={1}>
      {/* Thinking block */}
      {thinking && thinking.trim() && (
        <Box 
          borderStyle="round" 
          borderColor={colors.thinking}
          paddingX={1}
        >
          <Text color={colors.thinking} dimColor>
            {zen.arrow} Thinking: {thinking.slice(-200)}
            {thinking.length > 200 ? '...' : ''}
          </Text>
        </Box>
      )}

      {/* Tool indicators */}
      {tools.length > 0 && (
        <Box flexDirection="column">
          {tools.map((tool, idx) => (
            <ToolIndicator key={idx} tool={tool} />
          ))}
        </Box>
      )}

      {/* Streamed content */}
      {content && content.trim() && (
        <Box flexDirection="column">
          <Text color={colors.role.assistant}>
            BaoClaw {zen.separator}
          </Text>
          <Box paddingX={1}>
            <Text color={colors.text.primary}>
              {content}
            </Text>
          </Box>
        </Box>
      )}
    </Box>
  );
};

export default StreamOutput;
