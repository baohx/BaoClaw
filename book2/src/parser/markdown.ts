/**
 * Markdown Parser
 * 
 * 解析 Markdown 文件并提取章节结构、代码块、图片引用、外部链接等元素
 * 支持自定义 Markdown 扩展语法（源文件路径标注）
 */

import { readFile, readdir, stat } from 'fs/promises';
import { join, basename } from 'path';
import { marked, Token, Tokens } from 'marked';
import type {
  ParsedChapter,
  ChapterSections,
  Section,
  CodeBlock,
  Asset,
  ExternalLink,
} from '../types';

// 章节标题映射
const SECTION_HEADERS: Record<string, keyof ChapterSections> = {
  '问题': 'problem',
  '模式': 'pattern',
  '实现': 'implementation',
  '思考': 'reflection',
  '总结': 'summary',
};

// 必需的章节部分
export const REQUIRED_SECTIONS = ['problem', 'pattern', 'implementation', 'reflection'];

/**
 * Markdown 解析器
 * 负责解析 Markdown 文件并提取章节结构
 */
export class MarkdownParser {
  private codeBlockCounter = 0;
  private chapterCounter = 0;

  /**
   * 解析单个 Markdown 文件
   * @param path 文件路径
   * @param order 章节顺序（可选）
   * @returns 解析后的章节结构
   */
  async parseFile(path: string, order?: number): Promise<ParsedChapter> {
    const content = await readFile(path, 'utf-8');
    return this.parseContent(content, path, order);
  }

  /**
   * 解析 Markdown 内容
   * @param content Markdown 内容
   * @param filePath 文件路径（用于生成 ID）
   * @param order 章节顺序（可选）
   * @returns 解析后的章节结构
   */
  parseContent(content: string, filePath: string, order?: number): ParsedChapter {
    this.codeBlockCounter = 0;
    
    // 从文件路径提取章节 ID
    const id = this.extractChapterId(filePath);
    const chapterOrder = order ?? this.chapterCounter++;
    
    // 使用 marked 解析
    const tokens = marked.lexer(content);
    
    // 提取章节标题（第一个 h1）
    const title = this.extractTitle(tokens);
    
    // 提取各个部分
    const sections = this.extractSections(tokens);
    
    // 提取代码块
    const codeBlocks = this.extractCodeBlocks(tokens);
    
    // 提取资源（图片）
    const assets = this.extractAssets(tokens);
    
    // 提取外部链接
    const externalLinks = this.extractExternalLinks(tokens);

    return {
      id,
      order: chapterOrder,
      title,
      sections,
      codeBlocks,
      assets,
      externalLinks,
    };
  }

  /**
   * 解析目录下所有 Markdown 文件
   * @param dir 目录路径
   * @returns 解析后的章节数组
   */
  async parseDirectory(dir: string): Promise<ParsedChapter[]> {
    const chapters: ParsedChapter[] = [];
    const entries = await readdir(dir, { withFileTypes: true });
    
    // 按目录名排序（假设目录名格式为 XX-name）
    const chapterDirs = entries
      .filter(entry => entry.isDirectory() && /^\d{2}-/.test(entry.name))
      .sort((a, b) => a.name.localeCompare(b.name));

    for (let i = 0; i < chapterDirs.length; i++) {
      const chapterDir = chapterDirs[i];
      const readmePath = join(dir, chapterDir.name, 'README.md');
      
      try {
        await stat(readmePath);
        const chapter = await this.parseFile(readmePath, i);
        chapters.push(chapter);
      } catch {
        // README.md 不存在，跳过
        console.warn(`Warning: No README.md found in ${chapterDir.name}`);
      }
    }

    return chapters;
  }

  /**
   * 从文件路径提取章节 ID
   */
  private extractChapterId(filePath: string): string {
    const base = basename(filePath);
    if (base === 'README.md') {
      const dirName = basename(join(filePath, '..'));
      return dirName;
    }
    return base.replace(/\.md$/, '');
  }

  /**
   * 提取章节标题
   */
  private extractTitle(tokens: Token[]): string {
    for (const token of tokens) {
      if (token.type === 'heading' && token.depth === 1) {
        return token.text;
      }
    }
    return 'Untitled Chapter';
  }

  /**
   * 提取各部分内容
   */
  private extractSections(tokens: Token[]): ChapterSections {
    const sections: ChapterSections = {};
    let currentSection: keyof ChapterSections | null = null;
    let sectionStartLine = 1;
    let sectionTokens: Token[] = [];

    for (let i = 0; i < tokens.length; i++) {
      const token = tokens[i];
      
      if (token.type === 'heading' && token.depth === 2) {
        // 保存前一个部分
        if (currentSection !== null && sectionTokens.length > 0) {
          sections[currentSection] = this.createSection(
            this.getSectionTitle(currentSection),
            sectionTokens,
            sectionStartLine
          );
        }
        
        // 检查是否是已知的部分标题
        const sectionKey = SECTION_HEADERS[token.text];
        if (sectionKey) {
          currentSection = sectionKey;
          sectionStartLine = i;
          sectionTokens = [];
        } else {
          currentSection = null;
          sectionTokens = [];
        }
      } else if (currentSection !== null) {
        sectionTokens.push(token);
      }
    }

    // 保存最后一个部分
    if (currentSection !== null && sectionTokens.length > 0) {
      sections[currentSection] = this.createSection(
        this.getSectionTitle(currentSection),
        sectionTokens,
        sectionStartLine
      );
    }

    return sections;
  }

  /**
   * 获取部分标题
   */
  private getSectionTitle(key: keyof ChapterSections): string {
    const titles: Record<keyof ChapterSections, string> = {
      problem: '问题',
      pattern: '模式',
      implementation: '实现',
      reflection: '思考',
      summary: '总结',
    };
    return titles[key];
  }

  /**
   * 创建部分对象
   */
  private createSection(title: string, tokens: Token[], lineNumber: number): Section {
    // 将 tokens 转换回 Markdown 字符串
    const content = this.tokensToMarkdown(tokens);
    
    return {
      title,
      content,
      lineNumber,
    };
  }

  /**
   * 将 tokens 转换为 Markdown 字符串
   */
  private tokensToMarkdown(tokens: Token[]): string {
    return tokens.map(token => {
      if ('raw' in token && typeof token.raw === 'string') {
        return token.raw;
      }
      // 对于没有 raw 的 token，尝试重建
      return this.reconstructToken(token);
    }).join('');
  }

  /**
   * 重建 token 为 Markdown
   */
  private reconstructToken(token: Token): string {
    switch (token.type) {
      case 'paragraph':
        return 'text' in token ? `${token.text}\n\n` : '\n\n';
      case 'heading':
        return 'text' in token ? `${'#'.repeat(token.depth)} ${token.text}\n\n` : '\n\n';
      case 'code':
        return 'text' in token ? `\`\`\`${'lang' in token && token.lang ? token.lang : ''}\n${token.text}\n\`\`\`\n\n` : '\n\n';
      case 'list':
        return 'text' in token ? `${token.text}\n` : '\n';
      case 'space':
        return '\n';
      default:
        return 'text' in token ? token.text || '' : '';
    }
  }

  /**
   * 提取代码块
   */
  private extractCodeBlocks(tokens: Token[]): CodeBlock[] {
    const codeBlocks: CodeBlock[] = [];
    
    this.extractCodeBlocksRecursive(tokens, codeBlocks);
    
    return codeBlocks;
  }

  /**
   * 递归提取代码块
   */
  private extractCodeBlocksRecursive(tokens: Token[], codeBlocks: CodeBlock[]): void {
    for (const token of tokens) {
      if (token.type === 'code') {
        const codeToken = token as Tokens.Code;
        const codeBlock = this.parseCodeBlock(codeToken);
        codeBlocks.push(codeBlock);
      }
      
      // 递归处理嵌套的 tokens
      if ('tokens' in token && Array.isArray(token.tokens)) {
        this.extractCodeBlocksRecursive(token.tokens, codeBlocks);
      }
      
      // 处理列表项
      if (token.type === 'list' && 'items' in token) {
        const listToken = token as Tokens.List;
        for (const item of listToken.items) {
          if (item.tokens) {
            this.extractCodeBlocksRecursive(item.tokens, codeBlocks);
          }
        }
      }
    }
  }

  /**
   * 解析代码块
   */
  private parseCodeBlock(token: Tokens.Code): CodeBlock {
    const id = `code-${++this.codeBlockCounter}`;
    
    // 解析语言
    let language: CodeBlock['language'] = 'other';
    if (token.lang) {
      const lang = token.lang.toLowerCase().split(' ')[0];
      if (['rust', 'typescript', 'bash', 'mermaid'].includes(lang)) {
        language = lang as CodeBlock['language'];
      }
    }
    
    // 解析自定义元数据（path, lines）
    const { sourcePath, lineRange } = this.parseCodeMetadata(token.lang || '');
    
    return {
      id,
      language,
      code: token.text,
      sourcePath,
      lineRange,
    };
  }

  /**
   * 解析代码块元数据
   * 支持格式: ```rust path="src/main.rs" lines="10-20"
   */
  private parseCodeMetadata(langString: string): {
    sourcePath?: string;
    lineRange?: { start: number; end: number };
  } {
    const result: { sourcePath?: string; lineRange?: { start: number; end: number } } = {};
    
    // 提取 path 属性
    const pathMatch = langString.match(/path="([^"]+)"/);
    if (pathMatch) {
      result.sourcePath = pathMatch[1];
    }
    
    // 提取 lines 属性
    const linesMatch = langString.match(/lines="(\d+)-(\d+)"/);
    if (linesMatch) {
      result.lineRange = {
        start: parseInt(linesMatch[1], 10),
        end: parseInt(linesMatch[2], 10),
      };
    }
    
    return result;
  }

  /**
   * 提取资源（图片）
   */
  private extractAssets(tokens: Token[]): Asset[] {
    const assets: Asset[] = [];
    
    this.extractAssetsRecursive(tokens, assets);
    
    return assets;
  }

  /**
   * 递归提取资源
   */
  private extractAssetsRecursive(tokens: Token[], assets: Asset[]): void {
    for (const token of tokens) {
      // 处理图片
      if (token.type === 'image') {
        const imageToken = token as Tokens.Image;
        assets.push({
          type: 'image',
          path: imageToken.href,
          alt: imageToken.text,
        });
      }
      
      // 递归处理嵌套的 tokens
      if ('tokens' in token && Array.isArray(token.tokens)) {
        this.extractAssetsRecursive(token.tokens, assets);
      }
      
      // 处理链接中的图片
      if (token.type === 'link' && 'tokens' in token) {
        const linkToken = token as Tokens.Link;
        if (Array.isArray(linkToken.tokens)) {
          this.extractAssetsRecursive(linkToken.tokens, assets);
        }
      }
      
      // 处理列表项
      if (token.type === 'list' && 'items' in token) {
        const listToken = token as Tokens.List;
        for (const item of listToken.items) {
          if (item.tokens) {
            this.extractAssetsRecursive(item.tokens, assets);
          }
        }
      }
    }
  }

  /**
   * 提取外部链接
   */
  private extractExternalLinks(tokens: Token[]): ExternalLink[] {
    const links: ExternalLink[] = [];
    const seenUrls = new Set<string>();
    
    this.extractLinksRecursive(tokens, links, seenUrls);
    
    return links;
  }

  /**
   * 递归提取链接
   */
  private extractLinksRecursive(
    tokens: Token[], 
    links: ExternalLink[], 
    seenUrls: Set<string>
  ): void {
    for (const token of tokens) {
      // 处理链接
      if (token.type === 'link') {
        const linkToken = token as Tokens.Link;
        const url = linkToken.href;
        
        // 只处理外部链接（http/https）
        if (url.startsWith('http://') || url.startsWith('https://')) {
          // 去重
          if (!seenUrls.has(url)) {
            seenUrls.add(url);
            
            // 判断链接类型
            let type: ExternalLink['type'] = 'reference';
            if (url.includes('github.com')) {
              type = 'github';
            } else if (url.includes('docs.') || url.includes('/docs/') || url.endsWith('.md')) {
              type = 'docs';
            }
            
            links.push({
              url,
              label: linkToken.text,
              type,
            });
          }
        }
      }
      
      // 递归处理嵌套的 tokens
      if ('tokens' in token && Array.isArray(token.tokens)) {
        this.extractLinksRecursive(token.tokens, links, seenUrls);
      }
      
      // 处理列表项
      if (token.type === 'list' && 'items' in token) {
        const listToken = token as Tokens.List;
        for (const item of listToken.items) {
          if (item.tokens) {
            this.extractLinksRecursive(item.tokens, links, seenUrls);
          }
        }
      }
    }
  }
}
