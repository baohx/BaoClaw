// TUI Theme - 极客禅宗风格
// High contrast colors for dark terminal backgrounds

export const colors = {
  // Primary text - high contrast
  text: {
    primary: '#FFFFFF',      // White for main text
    secondary: '#E0E0E0',    // Light gray
    dim: '#A0A0A0',          // Medium gray for less important
    muted: '#808080',        // Darker gray
  },
  
  // Status colors
  status: {
    success: '#00FF00',      // Bright green
    error: '#FF4444',        // Bright red
    warning: '#FFD700',      // Gold
    info: '#00BFFF',         // Deep sky blue
    streaming: '#00FFFF',    // Cyan
  },
  
  // Role colors
  role: {
    user: '#FFD700',         // Gold for user
    assistant: '#00FFFF',    // Cyan for assistant
    system: '#808080',       // Gray for system
  },
  
  // Thinking/tool colors
  thinking: '#9370DB',       // Medium purple
  tool: '#FFA500',          // Orange
  
  // UI elements
  border: '#4A4A4A',
  background: '#1A1A1A',
  
  // Markdown highlight colors (for future use)
  markdown: {
    codeBg: '#2A2A2A',
    keyword: '#FF79C6',
    string: '#F1FA8C',
    comment: '#6272A4',
    fn: '#50FA7B',
    type: '#8BE9FD',
    number: '#BD93F9',
  },
  
  // Timing colors
  timing: {
    fast: '#00FF00',
    medium: '#FFD700',
    slow: '#FF6B6B',
  },
};

// Zen-style separators and symbols
export const zen = {
  separator: '│',
  horizontal: '─',
  corner: {
    topLeft: '╭',
    topRight: '╮',
    bottomLeft: '╰',
    bottomRight: '╯',
  },
  arrow: '→',
  bullet: '•',
  check: '✓',
  cross: '✗',
  loading: '◐◓◑◒',
  sparkline: '▁▂▃▄▅▆▇█',
};
