import React from "react";
import { Text, Box } from "ink";
import { colors, zen } from "../theme.js";
import { Session } from "../types.js";

interface StatusBarProps {
  session: Session | null;
  isStreaming: boolean;
}

export const StatusBar: React.FC<StatusBarProps> = ({
  session,
  isStreaming,
}) => {
  const statusColor = isStreaming
    ? colors.status.streaming
    : session?.status === "error"
      ? colors.status.error
      : colors.status.success;

  const statusText = isStreaming
    ? "◐ Streaming"
    : session?.status === "error"
      ? "✗ Error"
      : "● Ready";

  return (
    <Box
      width="100%"
      paddingX={1}
      borderStyle="single"
      borderColor={colors.border}
    >
      {/* Session ID */}
      <Box width={12}>
        {session && (
          <>
            <Text color={colors.text.dim}>{session.id.slice(0, 8)}</Text>
            <Text color={colors.text.muted}>{zen.separator}</Text>
          </>
        )}
      </Box>

      {/* Model name */}
      <Box flexGrow={1}>
        <Text color={colors.status.info}>{session?.model || "No Model"}</Text>
      </Box>

      {/* Status indicator */}
      <Box width={12} justifyContent="flex-end">
        <Text color={statusColor}>{statusText}</Text>
      </Box>
    </Box>
  );
};

export default StatusBar;
