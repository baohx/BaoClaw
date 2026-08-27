import React from "react";
import { Text, Box, Static } from "ink";
import { colors, zen } from "../theme.js";
import { Message, ContentBlock } from "../types.js";

interface MessageListProps {
  messages: Message[];
}

const ContentBlockView: React.FC<{ block: ContentBlock }> = ({ block }) => {
  switch (block.type) {
    case "text":
      return <Text color={colors.text.primary}>{block.content}</Text>;

    case "thinking":
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

    case "tool_use": {
      const toolName = block.toolName || "tool";
      const inputStr = block.content || "{}";
      const inputPreview =
        inputStr.length > 200 ? inputStr.slice(0, 200) + "..." : inputStr;
      return (
        <Box flexDirection="column" marginY={0} paddingX={1}>
          <Box>
            <Text color={colors.tool} bold>
              {zen.arrow}{" "}
            </Text>
            <Text color={colors.tool} bold>
              {toolName}
            </Text>
            <Text color={colors.text.muted}> (running)</Text>
          </Box>
          {inputPreview && inputPreview !== "{}" && (
            <Box paddingX={2}>
              <Text color={colors.text.dim}>
                {inputPreview.split("\n").slice(0, 5).join("\n")}
                {inputPreview.split("\n").length > 5 ? "\n  ..." : ""}
              </Text>
            </Box>
          )}
        </Box>
      );
    }

    case "tool_result": {
      const isError = block.isError === true;
      const output = block.content || "";
      const outputPreview =
        output.length > 300 ? output.slice(0, 300) + "..." : output;
      const lines = outputPreview.split("\n");
      const preview =
        lines.slice(0, 8).join("\n") + (lines.length > 8 ? "\n  ..." : "");

      return (
        <Box flexDirection="column" paddingX={2} marginY={0}>
          <Box>
            <Text color={isError ? colors.status.error : colors.status.success}>
              {isError ? zen.cross : zen.check}{" "}
            </Text>
            <Text color={isError ? colors.status.error : colors.text.dim}>
              {isError ? "Error" : "Result"}
              {block.toolName ? ` (${block.toolName})` : ""}
            </Text>
          </Box>
          {preview && (
            <Box paddingX={2}>
              <Text color={isError ? colors.status.error : colors.text.dim}>
                {preview}
              </Text>
            </Box>
          )}
        </Box>
      );
    }

    case "code":
      return (
        <Box flexDirection="column" paddingX={1}>
          <Text color={colors.markdown.keyword}>
            {zen.arrow} Code ({block.language || "text"}):
          </Text>
          <Text color={colors.markdown.string}>
            {block.content.split("\n").slice(0, 10).join("\n")}
            {block.content.split("\n").length > 10 ? "\n..." : ""}
          </Text>
        </Box>
      );

    default:
      return null;
  }
};

const MessageView: React.FC<{ message: Message }> = ({ message }) => {
  const roleColor =
    message.role === "user" ? colors.role.user : colors.role.assistant;

  const roleLabel = message.role === "user" ? "You" : "BaoClaw";

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

  // Completed messages render once via <Static>: Ink writes them permanently
  // to the screen and never re-measures them on subsequent frames. This keeps
  // render cost O(new content) instead of O(full history) per stream chunk.
  // The empty-state and trailing spacer stay outside Static (dynamic region).
  return (
    <Box flexDirection="column">
      <Static items={messages}>
        {(msg) => <MessageView key={msg.id} message={msg} />}
      </Static>
      <Box flexGrow={1} />
    </Box>
  );
};

export default MessageList;
