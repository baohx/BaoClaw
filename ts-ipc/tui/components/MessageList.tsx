import React from 'react';
import { Text, Box } from 'ink';
import { colors, zen } from '../theme.js';
import { Message, ContentBlock } from '../types.js';

interface MessageListProps {
  messages: Message[];
}

const ContentBlockView: React.FC<{ block: ContentBlock }> = ({ block }) => {
  switch (block.type) {
    case 'text':
      return (
        <Text color={colors.text.primary}>
          {block.content}
        </Text>
      );

    case 'thinking':
      return (
        <Box flexDirection="column" paddingX={1}>
          <Text color={colors.thinking} dimColor>
            {zen.arrow} Thinking:
          </Text>
          <Text color={colors.text.dim} dimColor>
            {block.content}
          </Text>
        </Box>
      );

    case 'tool_use':
      return (
        <Box paddingX={1}>
          <Text color={colors.tool}>
            {zen.arrow} [{block.toolName || 'tool'}]
          </Text>
        </Box>
      );

    case 'tool_result':
      return (
        <Box paddingX={2}>
          <Text color={colors.text.dim}>
            {zen.check} {block.content.slice(0, 100)}
            {block.content.length > 100 ? '...' : ''}
          </Text>
        </Box>
      );

    case 'code':
      return (
        <Box flexDirection="column" paddingX={1}>
          <Text color={colors.markdown.keyword}>
            {zen.arrow} Code ({block.language || 'text'}):
          </Text>
          <Text color={colors.markdown.string}>
            {block.content.split('\n').slice(0, 10).join('\n')}
            {block.content.split('\n').length > 10 ? '\n...' : ''}
          </Text>
        </Box>
      );

    default:
      return null;
  }
};

const MessageView: React.FC<{ message: Message }> = ({ message }) => {
  const roleColor = message.role === 'user' 
    ? colors.role.user 
    : colors.role.assistant;

  const roleLabel = message.role === 'user' 
    ? 'You' 
    : 'BaoClaw';

  return (
    <Box flexDirection="column" marginY={1}>
      {/* Role header */}
      <Box>
        <Text color={roleColor} bold>
          {roleLabel}
        </Text>
        <Text color={colors.text.muted}> {zen.separator} </Text>
        <Text color={colors.text.dim}>
          {message.timestamp.toLocaleTimeString()}
        </Text>
      </Box>

      {/* Content */}
      <Box flexDirection="column" paddingX={1}>
        {message.content.map((block, idx) => (
          <ContentBlockView key={idx} block={block} />
        ))}
      </Box>
    </Box>
  );
};

export const MessageList: React.FC<MessageListProps> = ({ messages }) => {
  if (messages.length === 0) {
    return (
      <Box flexGrow={1} justifyContent="center" alignItems="center">
        <Text color={colors.text.dim}>
          Start a conversation with BaoClaw...
        </Text>
      </Box>
    );
  }

  return (
    <Box flexDirection="column" flexGrow={1}>
      {messages.map((msg) => (
        <MessageView key={msg.id} message={msg} />
      ))}
    </Box>
  );
};

export default MessageList;
