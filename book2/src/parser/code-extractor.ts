/**
 * Code Extractor Module
 * 
 * 提取 Rust/TypeScript 代码块并解析代码块元数据（path, lines）
 * 
 * 支持的 Markdown 语法扩展：
 * ```rust path="baoclaw-core/src/engine/tool_executor.rs" lines="45-78"
 * fn execute_tool(...) { ... }
 * ```
 * 
 * Requirements: 3.1, 3.2
 */

import type { CodeBlock } from '../types';

/**
 * 代码块元数据接口
 */
interface CodeBlockMetadata {
  path?: string;
  lines?: { start: number; end: number };
}

/**
 * 代码块提取器
 * 
 * 从 Markdown 内容中提取代码块，支持自定义元数据解析
 */
export class CodeExtractor {
  /**
   * 代码块正则表达式
   * 匹配 ```lang meta\ncode\n``` 格式
   * 语言标识符是可选的，支持 ``` 或 ```lang
   * 
   * Group 1: 语言标识符（可选，可能是空字符串）
   * Group 2: 元数据字符串（path, lines 等）
   * Group 3: 代码内容
   * 
   * 使用非贪婪匹配，确保正确解析各种格式
   */
  private static readonly CODE_BLOCK_REGEX = /```([a-zA-Z]*)([^\n]*)\n([\s\S]*?)```/g;

  /**
   * 从 Markdown 内容中提取所有代码块
   * 
   * @param content - Markdown 内容
   * @returns 代码块数组
   */
  extractCodeBlocks(content: string): CodeBlock[] {
    const codeBlocks: CodeBlock[] = [];
    let match: RegExpExecArray | null;
    let index = 0;

    // 重置正则表达式的 lastIndex
    CodeExtractor.CODE_BLOCK_REGEX.lastIndex = 0;

    while ((match = CodeExtractor.CODE_BLOCK_REGEX.exec(content)) !== null) {
      const language = this.parseLanguage(match[1]);
      const metadataStr = match[2].trim();
      const code = match[3];
      const metadata = this.parseMetadata(metadataStr);

      const codeBlock: CodeBlock = {
        id: this.generateId(index),
        language,
        code: code.trim(),
      };

      // 添加可选的元数据
      if (metadata.path) {
        codeBlock.sourcePath = metadata.path;
      }
      if (metadata.lines) {
        codeBlock.lineRange = metadata.lines;
      }

      codeBlocks.push(codeBlock);
      index++;
    }

    return codeBlocks;
  }

  /**
   * 解析语言标识符
   * 
   * @param lang - 语言字符串（可能为空）
   * @returns 规范化的语言类型
   */
  private parseLanguage(lang: string): CodeBlock['language'] {
    // 空字符串返回 'other'
    if (!lang || lang.trim() === '') {
      return 'other';
    }
    
    const normalized = lang.toLowerCase();
    
    switch (normalized) {
      case 'rust':
      case 'rs':
        return 'rust';
      case 'typescript':
      case 'ts':
        return 'typescript';
      case 'bash':
      case 'sh':
      case 'shell':
        return 'bash';
      case 'mermaid':
        return 'mermaid';
      default:
        return 'other';
    }
  }

  /**
   * 解析代码块元数据
   * 
   * 支持的元数据格式：
   * - path="baoclaw-core/src/engine/query.rs"
   * - lines="45-78"
   * - path="..." lines="..."
   * 
   * @param metadataStr - 元数据字符串
   * @returns 解析后的元数据对象
   */
  private parseMetadata(metadataStr: string): CodeBlockMetadata {
    const metadata: CodeBlockMetadata = {};

    if (!metadataStr) {
      return metadata;
    }

    // 解析 path 属性
    const pathMatch = metadataStr.match(/path="([^"]+)"/);
    if (pathMatch) {
      metadata.path = pathMatch[1];
    }

    // 解析 lines 属性
    const linesMatch = metadataStr.match(/lines="(\d+)-(\d+)"/);
    if (linesMatch) {
      const start = parseInt(linesMatch[1], 10);
      const end = parseInt(linesMatch[2], 10);
      
      if (!isNaN(start) && !isNaN(end) && start > 0 && end >= start) {
        metadata.lines = { start, end };
      }
    }

    return metadata;
  }

  /**
   * 生成代码块唯一标识符
   * 
   * @param index - 代码块索引
   * @returns 唯一标识符
   */
  private generateId(index: number): string {
    return `code-block-${index}`;
  }

  /**
   * 解析 BaoClaw 源文件路径并验证
   * 
   * @param codeBlock - 代码块对象
   * @param baoclawRoot - BaoClaw 项目根目录
   * @returns 验证后的完整路径，如果无效则返回 null
   */
  resolveSourcePath(codeBlock: CodeBlock, baoclawRoot: string): string | null {
    if (!codeBlock.sourcePath) {
      return null;
    }

    // 构建完整路径
    const fullPath = `${baoclawRoot}/${codeBlock.sourcePath}`;
    
    return fullPath;
  }

  /**
   * 验证代码块元数据的有效性
   * 
   * @param codeBlock - 代码块对象
   * @returns 验证结果
   */
  validateMetadata(codeBlock: CodeBlock): { valid: boolean; errors: string[] } {
    const errors: string[] = [];

    // 验证 path 格式
    if (codeBlock.sourcePath) {
      // 路径不应该以 / 开头（应该是相对于项目根目录的相对路径）
      if (codeBlock.sourcePath.startsWith('/')) {
        errors.push(`Source path should be relative, got: ${codeBlock.sourcePath}`);
      }

      // 路径应该包含合理的文件扩展名
      const validExtensions = ['.rs', '.ts', '.tsx', '.js', '.jsx'];
      const hasValidExtension = validExtensions.some(ext => 
        codeBlock.sourcePath!.endsWith(ext)
      );
      
      if (!hasValidExtension) {
        errors.push(`Source path should have a valid extension (${validExtensions.join(', ')}), got: ${codeBlock.sourcePath}`);
      }
    }

    // 验证 lineRange
    if (codeBlock.lineRange) {
      const { start, end } = codeBlock.lineRange;
      
      if (start < 1) {
        errors.push(`Line range start must be >= 1, got: ${start}`);
      }
      
      if (end < start) {
        errors.push(`Line range end (${end}) must be >= start (${start})`);
      }
    }

    return {
      valid: errors.length === 0,
      errors,
    };
  }

  /**
   * 提取指定语言的代码块
   * 
   * @param content - Markdown 内容
   * @param language - 目标语言
   * @returns 匹配的代码块数组
   */
  extractByLanguage(content: string, language: CodeBlock['language']): CodeBlock[] {
    const allBlocks = this.extractCodeBlocks(content);
    return allBlocks.filter(block => block.language === language);
  }

  /**
   * 提取带有源文件路径的代码块
   * 
   * @param content - Markdown 内容
   * @returns 带有源文件路径的代码块数组
   */
  extractWithSourcePath(content: string): CodeBlock[] {
    const allBlocks = this.extractCodeBlocks(content);
    return allBlocks.filter(block => block.sourcePath !== undefined);
  }

  /**
   * 统计代码块信息
   * 
   * @param content - Markdown 内容
   * @returns 统计信息
   */
  getStatistics(content: string): {
    total: number;
    byLanguage: Record<CodeBlock['language'], number>;
    withSourcePath: number;
    withLineRange: number;
  } {
    const blocks = this.extractCodeBlocks(content);
    
    const byLanguage: Record<CodeBlock['language'], number> = {
      rust: 0,
      typescript: 0,
      bash: 0,
      mermaid: 0,
      other: 0,
    };

    let withSourcePath = 0;
    let withLineRange = 0;

    for (const block of blocks) {
      byLanguage[block.language]++;
      if (block.sourcePath) withSourcePath++;
      if (block.lineRange) withLineRange++;
    }

    return {
      total: blocks.length,
      byLanguage,
      withSourcePath,
      withLineRange,
    };
  }
}
