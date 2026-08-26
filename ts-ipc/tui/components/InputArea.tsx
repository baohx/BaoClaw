import React, { useState, useCallback } from "react";
import { Text, Box, useInput } from "ink";
import { colors, zen } from "../theme.js";

interface InputAreaProps {
  input: string;
  isStreaming: boolean;
  onSubmit: (text: string) => void;
  onInputChange: (text: string) => void;
}

export const InputArea: React.FC<InputAreaProps> = ({
  input,
  isStreaming,
  onSubmit,
  onInputChange,
}) => {
  const [cursorVisible, setCursorVisible] = useState(true);

  // Blink cursor
  React.useEffect(() => {
    const timer = setInterval(() => {
      setCursorVisible((v) => !v);
    }, 500);
    return () => clearInterval(timer);
  }, []);

  // Handle keyboard input
  useInput((inputChar, key) => {
    if (isStreaming) return;

    if (key.return) {
      if (input.trim()) {
        onSubmit(input.trim());
        onInputChange("");
      }
    } else if (key.backspace || key.delete) {
      onInputChange(input.slice(0, -1));
    } else if (!key.ctrl && !key.meta && inputChar) {
      onInputChange(input + inputChar);
    }
  });

  const displayText = input || "Type your message...";
  const displayColor = input ? colors.text.primary : colors.text.dim;

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      borderColor={colors.border}
      paddingX={1}
    >
      {/* Input hint */}
      <Box>
        <Text color={colors.role.user} bold>
          You
        </Text>
        <Text color={colors.text.muted}> {zen.separator} </Text>
        <Text color={displayColor}>{displayText}</Text>
        {cursorVisible && input && <Text color={colors.text.primary}>█</Text>}
      </Box>

      {/* Help text */}
      <Box>
        <Text color={colors.text.dim}>
          Press Enter to send • Ctrl+C to exit
        </Text>
      </Box>
    </Box>
  );
};

export default InputArea;
