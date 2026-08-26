import React from "react";
import { Text, Box } from "ink";
import { colors, zen } from "../theme.js";

interface HelpOverlayProps {
  visible: boolean;
  onClose: () => void;
}

const shortcuts = [
  { key: "Enter", action: "Send message" },
  { key: "Ctrl+C", action: "Exit" },
  { key: "Ctrl+H", action: "Toggle help" },
  { key: "Backspace", action: "Delete character" },
];

export const HelpOverlay: React.FC<HelpOverlayProps> = ({
  visible,
  onClose,
}) => {
  if (!visible) return null;

  return (
    <Box flexDirection="column" width="100%" height="100%" padding={2}>
      <Box marginBottom={1}>
        <Text color={colors.status.info} bold>
          Keyboard Shortcuts
        </Text>
      </Box>

      {shortcuts.map((s, idx) => (
        <Box key={idx} marginBottom={1}>
          <Box width={12}>
            <Text color={colors.role.user}>{s.key}</Text>
          </Box>
          <Text color={colors.text.primary}>
            {zen.arrow} {s.action}
          </Text>
        </Box>
      ))}

      <Box marginTop={2}>
        <Text color={colors.text.dim}>Press any key to close</Text>
      </Box>
    </Box>
  );
};

export default HelpOverlay;
