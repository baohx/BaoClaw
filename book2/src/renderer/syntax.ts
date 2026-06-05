/**
 * Syntax Highlighter Module
 * 
 * 集成 highlight.js
 * 支持 Rust 语法高亮
 * 支持 TypeScript 语法高亮
 * 
 * Requirements: 4.5
 */

import hljs from 'highlight.js';

/**
 * 支持的语言列表
 */
const SUPPORTED_LANGUAGES = ['rust', 'typescript', 'bash', 'javascript', 'json', 'markdown', 'plaintext'];

/**
 * 语法高亮器
 * 
 * 使用 highlight.js 对代码进行语法高亮
 */
export class SyntaxHighlighter {
  private initialized = false;

  constructor() {
    this.initialize();
  }

  /**
   * 初始化高亮器
   */
  private initialize(): void {
    if (this.initialized) return;

    // highlight.js 已自动注册常见语言
    // 这里只注册我们需要的语言
    this.initialized = true;
  }

  /**
   * 高亮代码
   * 
   * @param code - 要高亮的代码
   * @param language - 语言标识符
   * @returns 高亮后的 HTML
   */
  highlight(code: string, language: string): string {
    // 规范化语言名称
    const normalizedLang = this.normalizeLanguage(language);

    try {
      if (SUPPORTED_LANGUAGES.includes(normalizedLang)) {
        const result = hljs.highlight(code, { language: normalizedLang });
        return result.value;
      }
    } catch (error) {
      console.warn(`Syntax highlighting failed for language: ${language}`, error);
    }

    // 回退到自动检测或纯文本
    try {
      const result = hljs.highlightAuto(code);
      return result.value;
    } catch {
      // 最后回退到 HTML 转义
      return this.escapeHtml(code);
    }
  }

  /**
   * 高亮代码块（带包装）
   * 
   * @param code - 要高亮的代码
   * @param language - 语言标识符
   * @param showLineNumbers - 是否显示行号（可选）
   * @returns 高亮后的完整 HTML 块
   */
  highlightBlock(code: string, language: string, showLineNumbers = false): string {
    const highlighted = this.highlight(code, language);
    const langClass = `language-${this.normalizeLanguage(language)}`;

    if (showLineNumbers) {
      const lines = highlighted.split('\n');
      const numberedLines = lines.map((line, index) => {
        const lineNum = index + 1;
        return `<span class="line" data-line="${lineNum}">${line}</span>`;
      }).join('\n');

      return `<pre class="code-block ${langClass} line-numbers"><code class="hljs">${numberedLines}</code></pre>`;
    }

    return `<pre class="code-block ${langClass}"><code class="hljs">${highlighted}</code></pre>`;
  }

  /**
   * 高亮页面中的所有代码块
   * 
   * @param container - 容器元素（默认为 document）
   */
  highlightAll(container: HTMLElement | Document = document): void {
    const codeBlocks = container.querySelectorAll('pre code');

    codeBlocks.forEach((block) => {
      const element = block as HTMLElement;
      
      // 跳过已高亮的代码块
      if (element.classList.contains('hljs')) {
        return;
      }

      // 尝试从 class 中提取语言
      const language = this.detectLanguageFromClass(element);
      const code = element.textContent || '';

      try {
        const highlighted = this.highlight(code, language);
        element.innerHTML = highlighted;
        element.classList.add('hljs');
      } catch (error) {
        console.warn('Failed to highlight code block:', error);
      }
    });
  }

  /**
   * 检测元素的语言
   */
  private detectLanguageFromClass(element: HTMLElement): string {
    const classes = element.className.split(/\s+/);
    
    for (const className of classes) {
      if (className.startsWith('language-')) {
        return className.replace('language-', '');
      }
    }

    // 默认自动检测
    return 'plaintext';
  }

  /**
   * 规范化语言名称
   */
  private normalizeLanguage(language: string): string {
    const langMap: Record<string, string> = {
      'ts': 'typescript',
      'js': 'javascript',
      'sh': 'bash',
      'shell': 'bash',
      'text': 'plaintext',
      '': 'plaintext',
    };

    const normalized = language.toLowerCase().trim();
    return langMap[normalized] || normalized;
  }

  /**
   * HTML 转义
   */
  private escapeHtml(text: string): string {
    const escapeMap: Record<string, string> = {
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      '"': '&quot;',
      "'": '&#39;',
    };

    return text.replace(/[&<>"']/g, char => escapeMap[char]);
  }
}

/**
 * 单例实例
 */
let syntaxHighlighterInstance: SyntaxHighlighter | null = null;

/**
 * 获取语法高亮器实例
 */
export function getSyntaxHighlighter(): SyntaxHighlighter {
  if (!syntaxHighlighterInstance) {
    syntaxHighlighterInstance = new SyntaxHighlighter();
  }
  return syntaxHighlighterInstance;
}

/**
 * 创建语法高亮器实例（用于测试）
 */
export function createSyntaxHighlighter(): SyntaxHighlighter {
  return new SyntaxHighlighter();
}

/**
 * 便捷函数：高亮代码
 */
export function highlightCode(code: string, language: string): string {
  return getSyntaxHighlighter().highlight(code, language);
}

/**
 * 便捷函数：高亮代码块
 */
export function highlightCodeBlock(code: string, language: string, showLineNumbers = false): string {
  return getSyntaxHighlighter().highlightBlock(code, language, showLineNumbers);
}
