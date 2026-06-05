/**
 * Renderer Module Index
 * 
 * 导出所有渲染器相关模块
 * 
 * Requirements: 4.1, 4.5, 4.6
 */

export { SlideRenderer, createSlideRenderer } from './slide-renderer';
export { ThemeManagerImpl, getThemeManager, createThemeManager } from './theme';
export { 
  SyntaxHighlighter, 
  getSyntaxHighlighter, 
  createSyntaxHighlighter,
  highlightCode,
  highlightCodeBlock 
} from './syntax';

// Re-export types for convenience
