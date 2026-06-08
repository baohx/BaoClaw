/**
 * 极客禅宗主题 - 高对比度优化版
 * 
 * 设计哲学：
 * - 黑色为虚空，象征无限可能
 * - 橙色为能量，象征行动与创造
 * - 高对比度确保可读性
 */

export const colors = {
  // 主色调 - BaoClaw 橙（提高亮度）
  primary: '#FF9500',
  primaryBright: '#FFB340',
  primaryDim: '#CC7700',
  
  // 背景色（不使用，终端自带背景）
  void: '#000000',
  voidLight: '#1A1A1A',
  voidMid: '#333333',
  
  // 文字层级 - 高对比度
  text: {
    primary: '#FFFFFF',      // 纯白，最高对比
    secondary: '#CCCCCC',    // 亮灰
    dim: '#888888',          // 中灰
    muted: '#666666',        // 暗灰
  },
  
  // 状态色 - 高饱和度
  status: {
    success: '#00FF88',      // 亮绿
    warning: '#FFD700',      // 金黄
    error: '#FF4444',        // 亮红
    info: '#00DDFF',         // 亮青
  },
  
  // 语义色 - 高对比
  semantic: {
    thinking: '#BB88FF',     // 亮紫
    tool: '#FF66AA',         // 亮粉
    user: '#FF9500',         // 橙色
    assistant: '#00DDFF',    // 亮青
    system: '#999999',       // 灰色
  },
  
  // ANSI 颜色（直接使用终端颜色，确保兼容）
  ansi: {
    brightWhite: '\x1b[97m',
    white: '\x1b[37m',
    brightCyan: '\x1b[96m',
    brightGreen: '\x1b[92m',
    brightYellow: '\x1b[93m',
    brightRed: '\x1b[91m',
    brightMagenta: '\x1b[95m',
    brightBlue: '\x1b[94m',
    cyan: '\x1b[36m',
    green: '\x1b[32m',
    yellow: '\x1b[33m',
    red: '\x1b[31m',
    magenta: '\x1b[35m',
    blue: '\x1b[34m',
    dim: '\x1b[2m',
    bold: '\x1b[1m',
    reset: '\x1b[0m',
  },
};

// Zen 符号
export const zen = {
  dot: '●',
  circle: '○',
  empty: '○',
  wave: '∿',
  infinity: '∞',
  spark: '✦',
  star: '✧',
  diamond: '◆',
  triangle: '△',
  arrow: '→',
  arrowDown: '↓',
  arrowUp: '↑',
  corner: '┌',
  cornerEnd: '└',
  line: '│',
  lineH: '─',
  lineD: '╌',
  separator: '·',
  // 呼吸动画符号
  breath: ['○', '◐', '●', '◐'],
  // 禅意装饰
  zenLine: '─────────── ∙ ───────────',
};

// 工具图标映射
export const toolIcons: Record<string, string> = {
  Bash: '⚡',
  FileRead: '▶',
  FileWrite: '✎',
  FileEdit: '✂',
  Grep: '◈',
  Glob: '◇',
  WebFetch: '◎',
  WebSearch: '◉',
  Agent: '◆',
  TodoWrite: '▣',
  Memory: '◎',
  default: '◆',
};

// Markdown 语法高亮色
export const markdown = {
  codeBg: '#1A1A1A',
  keyword: '#FF79C6',
  string: '#F1FA8C',
  comment: '#6272A4',
  fn: '#50FA7B',
  type: '#8BE9FD',
  number: '#BD93F9',
};

// 布局常量
export const layout = {
  minWidth: 60,
  minHeight: 20,
  inputHeight: 3,
  statusBarHeight: 1,
  paddingX: 1,
  paddingY: 0,
};

// 动画时序
export const timing = {
  spinner: 80,
  breath: 1000,
  fade: 200,
  pulse: 2000,
};
