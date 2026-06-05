/**
 * Slide Generator Module
 * 
 * 将解析后的章节转换为幻灯片数据结构
 * 生成幻灯片 ID 和进度信息
 * 
 * Requirements: 4.1
 */

import type {
  ParsedChapter,
  ChapterSections,
  Slide,
  SlideCollection,
  TableOfContents,
  ChapterEntry,
  SlideEntry,
  CodeBlock,
} from '../types';

/**
 * 幻灯片类型映射
 */
const SECTION_TO_SLIDE_TYPE: Record<keyof ChapterSections, Slide['type']> = {
  problem: 'problem',
  pattern: 'pattern',
  implementation: 'implementation',
  reflection: 'reflection',
  summary: 'summary',
};

/**
 * 幻灯片生成器
 * 
 * 负责将解析后的章节数据转换为幻灯片数据结构
 */
export class SlideGenerator {
  private slideCounter = 0;
  private totalSlides = 0;

  /**
   * 生成单个章节的幻灯片
   * 
   * @param chapter - 解析后的章节数据
   * @returns 幻灯片数组
   */
  generateChapter(chapter: ParsedChapter): Slide[] {
    const slides: Slide[] = [];
    this.slideCounter = 0;

    // 生成标题幻灯片
    slides.push(this.createTitleSlide(chapter));

    // 为每个部分生成幻灯片
    const sectionOrder: (keyof ChapterSections)[] = [
      'problem',
      'pattern',
      'implementation',
      'reflection',
      'summary',
    ];

    for (const sectionKey of sectionOrder) {
      const section = chapter.sections[sectionKey];
      if (section) {
        const sectionSlides = this.createSectionSlides(
          chapter,
          sectionKey,
          section
        );
        slides.push(...sectionSlides);
      }
    }

    return slides;
  }

  /**
   * 生成所有章节的幻灯片集合
   * 
   * @param chapters - 所有解析后的章节数组
   * @returns 幻灯片集合（包含目录）
   */
  generateAll(chapters: ParsedChapter[]): SlideCollection {
    // 首先计算总幻灯片数（用于进度计算）
    this.totalSlides = 0;
    for (const chapter of chapters) {
      this.totalSlides += this.estimateSlideCount(chapter);
    }

    const allSlides: Slide[] = [];
    const tocChapters: ChapterEntry[] = [];
    let processedSlides = 0;

    for (const chapter of chapters) {
      const chapterSlides = this.generateChapter(chapter);
      
      // 更新进度信息
      for (const slide of chapterSlides) {
        slide.progress = this.calculateProgress(
          processedSlides,
          this.totalSlides
        );
        processedSlides++;
      }

      allSlides.push(...chapterSlides);

      // 构建目录条目
      const tocEntry = this.createChapterEntry(chapter, chapterSlides);
      tocChapters.push(tocEntry);
    }

    // 重新计算进度（基于实际幻灯片数）
    const actualTotal = allSlides.length;
    for (let i = 0; i < allSlides.length; i++) {
      allSlides[i].progress = Math.round((i / actualTotal) * 100);
    }

    const tableOfContents: TableOfContents = {
      chapters: tocChapters,
    };

    return {
      slides: allSlides,
      tableOfContents,
      totalSlides: actualTotal,
    };
  }

  /**
   * 创建标题幻灯片
   */
  private createTitleSlide(chapter: ParsedChapter): Slide {
    const id = this.generateSlideId(chapter.id);

    return {
      id,
      chapterId: chapter.id,
      chapterTitle: chapter.title,
      title: chapter.title,
      content: this.generateTitleContent(chapter),
      type: 'title',
      progress: 0,
    };
  }

  /**
   * 生成标题幻灯片内容
   */
  private generateTitleContent(chapter: ParsedChapter): string {
    let content = `<h1>${this.escapeHtml(chapter.title)}</h1>\n`;
    
    // 添加章节序号
    if (chapter.order !== undefined) {
      const chapterNum = chapter.order + 1;
      content = `<div class="chapter-number">Chapter ${chapterNum}</div>\n${content}`;
    }

    return content;
  }

  /**
   * 为章节部分创建幻灯片
   */
  private createSectionSlides(
    chapter: ParsedChapter,
    sectionKey: keyof ChapterSections,
    section: { title: string; content: string; lineNumber: number }
  ): Slide[] {
    const slides: Slide[] = [];

    // 解析部分内容为多个幻灯片
    const contentSlides = this.parseSectionContent(
      chapter,
      sectionKey,
      section
    );

    slides.push(...contentSlides);

    return slides;
  }

  /**
   * 解析部分内容为幻灯片
   * 
   * 根据内容长度和代码块数量智能分割
   */
  private parseSectionContent(
    chapter: ParsedChapter,
    sectionKey: keyof ChapterSections,
    section: { title: string; content: string; lineNumber: number }
  ): Slide[] {
    const slides: Slide[] = [];

    // 检查是否有代码块
    const chapterCodeBlocks = chapter.codeBlocks.filter(() => {
      // 根据代码块在内容中的位置判断是否属于该部分
      return true; // 简化处理，所有代码块都可用于代码幻灯片
    });

    // 如果内容包含代码块，创建代码类型的幻灯片
    if (chapterCodeBlocks.length > 0 && sectionKey === 'implementation') {
      // 为代码块创建单独的幻灯片
      const codeSlides = this.createCodeSlides(
        chapter,
        section,
        chapterCodeBlocks
      );
      slides.push(...codeSlides);
    } else {
      // 创建普通内容幻灯片
      const contentSlide = this.createContentSlide(
        chapter,
        sectionKey,
        section
      );
      slides.push(contentSlide);
    }

    return slides;
  }

  /**
   * 创建内容幻灯片
   */
  private createContentSlide(
    chapter: ParsedChapter,
    sectionKey: keyof ChapterSections,
    section: { title: string; content: string; lineNumber: number }
  ): Slide {
    const id = this.generateSlideId(chapter.id);
    const slideType = SECTION_TO_SLIDE_TYPE[sectionKey];

    return {
      id,
      chapterId: chapter.id,
      chapterTitle: chapter.title,
      title: section.title,
      content: this.markdownToHtml(section.content),
      type: slideType,
      progress: 0, // 将在 generateAll 中更新
    };
  }

  /**
   * 创建代码幻灯片
   */
  private createCodeSlides(
    chapter: ParsedChapter,
    section: { title: string; content: string; lineNumber: number },
    codeBlocks: CodeBlock[]
  ): Slide[] {
    const slides: Slide[] = [];

    // 首先创建部分的标题幻灯片
    const titleSlide = this.createContentSlide(
      chapter,
      'implementation',
      section
    );
    slides.push(titleSlide);

    // 为每个代码块创建幻灯片
    for (const codeBlock of codeBlocks) {
      const id = this.generateSlideId(chapter.id);
      const codeSlide: Slide = {
        id,
        chapterId: chapter.id,
        chapterTitle: chapter.title,
        title: codeBlock.sourcePath 
          ? `代码示例: ${this.extractFileName(codeBlock.sourcePath)}`
          : '代码示例',
        content: this.generateCodeSlideContent(codeBlock),
        type: 'code',
        codeBlocks: [codeBlock],
        progress: 0,
      };
      slides.push(codeSlide);
    }

    return slides;
  }

  /**
   * 生成代码幻灯片内容
   */
  private generateCodeSlideContent(codeBlock: CodeBlock): string {
    let content = '';

    // 添加源文件路径信息
    if (codeBlock.sourcePath) {
      content += `<div class="code-source">`;
      content += `<span class="code-path">${this.escapeHtml(codeBlock.sourcePath)}</span>`;
      if (codeBlock.lineRange) {
        content += `<span class="code-lines">lines ${codeBlock.lineRange.start}-${codeBlock.lineRange.end}</span>`;
      }
      content += `</div>\n`;
    }

    // 添加代码块
    const langClass = this.getLanguageClass(codeBlock.language);
    content += `<pre class="code-block ${langClass}">`;
    content += `<code class="language-${codeBlock.language}">`;
    content += this.escapeHtml(codeBlock.code);
    content += `</code></pre>\n`;

    return content;
  }

  /**
   * 提取文件名
   */
  private extractFileName(path: string): string {
    const parts = path.split('/');
    return parts[parts.length - 1];
  }

  /**
   * 获取语言 CSS 类
   */
  private getLanguageClass(language: CodeBlock['language']): string {
    return `language-${language}`;
  }

  /**
   * 估算章节的幻灯片数量
   */
  private estimateSlideCount(chapter: ParsedChapter): number {
    let count = 1; // 标题幻灯片

    // 计算每个部分的幻灯片数
    const sectionKeys: (keyof ChapterSections)[] = [
      'problem',
      'pattern',
      'implementation',
      'reflection',
      'summary',
    ];

    for (const key of sectionKeys) {
      if (chapter.sections[key]) {
        count += 1; // 每个部分至少一个幻灯片
        
        // 如果是实现部分且有代码块，添加额外的幻灯片
        if (key === 'implementation' && chapter.codeBlocks.length > 0) {
          count += chapter.codeBlocks.length;
        }
      }
    }

    return count;
  }

  /**
   * 计算进度百分比
   */
  private calculateProgress(current: number, total: number): number {
    if (total === 0) return 0;
    return Math.round((current / total) * 100);
  }

  /**
   * 生成幻灯片唯一标识符
   */
  private generateSlideId(chapterId: string): string {
    const slideNum = String(++this.slideCounter).padStart(2, '0');
    return `${chapterId}-slide-${slideNum}`;
  }

  /**
   * 创建目录章节条目
   */
  private createChapterEntry(
    chapter: ParsedChapter,
    slides: Slide[]
  ): ChapterEntry {
    // 提取章节中的所有部分
    const sections: string[] = [];
    if (chapter.sections.problem) sections.push('问题');
    if (chapter.sections.pattern) sections.push('模式');
    if (chapter.sections.implementation) sections.push('实现');
    if (chapter.sections.reflection) sections.push('思考');
    if (chapter.sections.summary) sections.push('总结');

    // 构建幻灯片条目列表
    const slideEntries: SlideEntry[] = slides.map(slide => ({
      id: slide.id,
      title: slide.title,
      type: slide.type,
    }));

    return {
      id: chapter.id,
      title: chapter.title,
      sections,
      slideCount: slides.length,
      slides: slideEntries,
    };
  }

  /**
   * 简单的 Markdown 转 HTML
   * 
   * 注意：这是一个简化实现，生产环境应使用 marked 等库
   */
  private markdownToHtml(markdown: string): string {
    let html = markdown;

    // 转换标题
    html = html.replace(/^### (.*$)/gm, '<h3>$1</h3>');
    html = html.replace(/^## (.*$)/gm, '<h2>$1</h2>');
    html = html.replace(/^# (.*$)/gm, '<h1>$1</h1>');

    // 转换粗体和斜体
    html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
    html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');

    // 转换行内代码
    html = html.replace(/`([^`]+)`/g, '<code>$1</code>');

    // 转换链接
    html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>');

    // 转换段落（双换行）
    html = html.split('\n\n').map(para => {
      if (para.trim() && 
          !para.startsWith('<h') && 
          !para.startsWith('<ul') &&
          !para.startsWith('<ol') &&
          !para.startsWith('<pre') &&
          !para.startsWith('<blockquote')) {
        return `<p>${para}</p>`;
      }
      return para;
    }).join('\n');

    return html;
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
