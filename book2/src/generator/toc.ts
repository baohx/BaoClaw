/**
 * TOCBuilder Module
 * 
 * 构建章节目录结构，生成导航链接
 * 
 * Validates: Requirements 4.4
 */

import type {
  ParsedChapter,
  TableOfContents,
  ChapterEntry,
  SlideEntry,
  Slide,
} from '../types';

/**
 * TOCBuilder 配置选项
 */
export interface TOCBuilderOptions {
  /**
   * 是否生成幻灯片链接
   * @default true
   */
  includeSlideLinks?: boolean;

  /**
   * URL 前缀（用于生成导航链接）
   * @default '#/'
   */
  urlPrefix?: string;

  /**
   * 是否按章节顺序排序
   * @default true
   */
  sortByOrder?: boolean;
}

/**
 * 导航链接接口
 */
export interface NavigationLink {
  /**
   * 链接 ID
   */
  id: string;

  /**
   * 显示标题
   */
  title: string;

  /**
   * URL hash 路径
   */
  url: string;

  /**
   * 章节索引（0-based）
   */
  chapterIndex: number;

  /**
   * 幻灯片索引（0-based，可选）
   */
  slideIndex?: number;

  /**
   * 是否为章节链接
   */
  isChapter: boolean;

  /**
   * 幻灯片类型（仅幻灯片链接有效）
   */
  slideType?: Slide['type'];
}

/**
 * TOCBuilder 类
 * 
 * 负责构建章节目录结构并生成导航链接
 */
export class TOCBuilder {
  private options: Required<TOCBuilderOptions>;

  /**
   * 创建 TOCBuilder 实例
   * @param options 配置选项
   */
  constructor(options: TOCBuilderOptions = {}) {
    this.options = {
      includeSlideLinks: options.includeSlideLinks ?? true,
      urlPrefix: options.urlPrefix ?? '#/',
      sortByOrder: options.sortByOrder ?? true,
    };
  }

  /**
   * 从解析后的章节数据构建目录结构
   * 
   * @param chapters 解析后的章节数组
   * @returns 目录结构
   */
  build(chapters: ParsedChapter[]): TableOfContents {
    // 可选：按顺序排序
    const sortedChapters = this.options.sortByOrder
      ? [...chapters].sort((a, b) => a.order - b.order)
      : chapters;

    const chapterEntries: ChapterEntry[] = sortedChapters.map((chapter, index) => {
      return this.buildChapterEntry(chapter, index);
    });

    return {
      chapters: chapterEntries,
    };
  }

  /**
   * 构建单个章节的目录条目
   * 
   * @param chapter 解析后的章节数据
   * @param index 章节索引
   * @returns 章节目录条目
   */
  buildChapterEntry(chapter: ParsedChapter, index: number): ChapterEntry {
    // 提取章节部分名称列表
    const sections = this.extractSectionNames(chapter);

    // 生成幻灯片条目列表
    const slides = this.generateSlideEntries(chapter, index);

    return {
      id: chapter.id,
      title: chapter.title,
      sections,
      slideCount: slides.length,
      slides,
    };
  }

  /**
   * 提取章节部分名称列表
   * 
   * @param chapter 解析后的章节数据
   * @returns 部分名称数组
   */
  private extractSectionNames(chapter: ParsedChapter): string[] {
    const sectionNames: string[] = [];
    const sectionOrder = ['problem', 'pattern', 'implementation', 'reflection', 'summary'] as const;
    const sectionTitles: Record<string, string> = {
      problem: '问题',
      pattern: '模式',
      implementation: '实现',
      reflection: '思考',
      summary: '总结',
    };

    for (const key of sectionOrder) {
      const section = chapter.sections[key];
      if (section && section.content.trim().length > 0) {
        sectionNames.push(sectionTitles[key]);
      }
    }

    return sectionNames;
  }

  /**
   * 生成幻灯片条目列表
   * 
   * @param chapter 解析后的章节数据
   * @param _chapterIndex 章节索引（未使用）
   * @returns 幻灯片条目数组
   */
  private generateSlideEntries(chapter: ParsedChapter, _chapterIndex: number): SlideEntry[] {
    const slides: SlideEntry[] = [];
    let slideIndex = 0;

    // 章节标题页
    slides.push({
      id: this.generateSlideId(chapter.id, slideIndex),
      title: chapter.title,
      type: 'title',
    });
    slideIndex++;

    // 根据章节部分生成幻灯片
    const sectionOrder = ['problem', 'pattern', 'implementation', 'reflection', 'summary'] as const;

    for (const key of sectionOrder) {
      const section = chapter.sections[key];
      if (section && section.content.trim().length > 0) {
        // 每个部分可能有多个幻灯片（如果包含代码块等）
        const sectionSlides = this.generateSectionSlides(chapter, key, slideIndex);
        slides.push(...sectionSlides);
        slideIndex += sectionSlides.length;
      }
    }

    return slides;
  }

  /**
   * 为章节部分生成幻灯片条目
   * 
   * @param chapter 章节数据
   * @param sectionKey 部分键名
   * @param startSlideIndex 起始幻灯片索引
   * @returns 幻灯片条目数组
   */
  private generateSectionSlides(
    chapter: ParsedChapter,
    sectionKey: keyof typeof chapter.sections,
    startSlideIndex: number
  ): SlideEntry[] {
    const slides: SlideEntry[] = [];
    const section = chapter.sections[sectionKey];

    if (!section) {
      return slides;
    }

    const slideType = this.getSlideType(sectionKey);

    // 主幻灯片
    slides.push({
      id: this.generateSlideId(chapter.id, startSlideIndex),
      title: section.title,
      type: slideType,
    });

    // 如果包含代码块，可以生成额外的代码幻灯片
    // 这里简化处理，每个部分生成一个幻灯片
    // 实际实现中可以根据内容长度和代码块数量拆分

    return slides;
  }

  /**
   * 获取幻灯片类型
   */
  private getSlideType(sectionKey: string): Slide['type'] {
    const typeMap: Record<string, Slide['type']> = {
      problem: 'problem',
      pattern: 'pattern',
      implementation: 'implementation',
      reflection: 'reflection',
      summary: 'summary',
    };
    return typeMap[sectionKey] || 'problem';
  }

  /**
   * 生成幻灯片 ID
   * 
   * @param chapterId 章节 ID
   * @param slideIndex 幻灯片索引
   * @returns 幻灯片 ID
   */
  private generateSlideId(chapterId: string, slideIndex: number): string {
    return `${chapterId}-${String(slideIndex).padStart(2, '0')}`;
  }

  /**
   * 生成所有导航链接
   * 
   * @param chapters 解析后的章节数组
   * @returns 导航链接数组
   */
  generateNavigationLinks(chapters: ParsedChapter[]): NavigationLink[] {
    const links: NavigationLink[] = [];
    const sortedChapters = this.options.sortByOrder
      ? [...chapters].sort((a, b) => a.order - b.order)
      : chapters;

    for (let chapterIndex = 0; chapterIndex < sortedChapters.length; chapterIndex++) {
      const chapter = sortedChapters[chapterIndex];

      // 章节链接
      links.push({
        id: chapter.id,
        title: chapter.title,
        url: this.generateChapterUrl(chapter.id),
        chapterIndex,
        isChapter: true,
      });

      // 如果配置为包含幻灯片链接
      if (this.options.includeSlideLinks) {
        const toc = this.build([chapter]);
        const chapterEntry = toc.chapters[0];

        for (let slideIndex = 0; slideIndex < chapterEntry.slides.length; slideIndex++) {
          const slide = chapterEntry.slides[slideIndex];

          // 跳过标题页（通常章节链接已经指向它）
          if (slideIndex === 0 && slide.type === 'title') {
            continue;
          }

          links.push({
            id: slide.id,
            title: slide.title,
            url: this.generateSlideUrl(chapter.id, slideIndex),
            chapterIndex,
            slideIndex,
            isChapter: false,
            slideType: slide.type,
          });
        }
      }
    }

    return links;
  }

  /**
   * 生成章节 URL
   * 
   * @param chapterId 章节 ID
   * @returns URL hash 路径
   */
  private generateChapterUrl(chapterId: string): string {
    return `${this.options.urlPrefix}${chapterId}`;
  }

  /**
   * 生成幻灯片 URL
   * 
   * @param chapterId 章节 ID
   * @param slideIndex 幻灯片索引
   * @returns URL hash 路径
   */
  private generateSlideUrl(chapterId: string, slideIndex: number): string {
    return `${this.options.urlPrefix}${chapterId}/${String(slideIndex).padStart(2, '0')}`;
  }

  /**
   * 从幻灯片 URL 解析位置信息
   * 
   * @param url URL hash 路径
   * @returns 位置信息（章节 ID 和幻灯片索引）
   */
  parseSlideUrl(url: string): { chapterId: string; slideIndex: number } | null {
    // 移除 URL 前缀
    const path = url.replace(this.options.urlPrefix, '');

    if (!path) {
      return null;
    }

    // 解析格式: chapterId 或 chapterId/slideIndex
    const parts = path.split('/');

    if (parts.length === 1) {
      return {
        chapterId: parts[0],
        slideIndex: 0,
      };
    }

    if (parts.length === 2) {
      const chapterId = parts[0];
      const slideIndex = parseInt(parts[1], 10);

      if (isNaN(slideIndex)) {
        return null;
      }

      return {
        chapterId,
        slideIndex,
      };
    }

    return null;
  }

  /**
   * 查找指定章节的所有幻灯片链接
   * 
   * @param links 所有导航链接
   * @param chapterId 章节 ID
   * @returns 该章节的幻灯片链接数组
   */
  findChapterSlideLinks(links: NavigationLink[], chapterId: string): NavigationLink[] {
    return links.filter(link => 
      link.id.startsWith(chapterId) && !link.isChapter
    );
  }

  /**
   * 获取相邻章节的导航链接
   * 
   * @param links 所有导航链接
   * @param currentChapterId 当前章节 ID
   * @returns 上一个和下一个章节链接
   */
  getAdjacentChapters(
    links: NavigationLink[],
    currentChapterId: string
  ): { prev: NavigationLink | null; next: NavigationLink | null } {
    const chapterLinks = links.filter(link => link.isChapter);
    const currentIndex = chapterLinks.findIndex(link => link.id === currentChapterId);

    if (currentIndex === -1) {
      return { prev: null, next: null };
    }

    return {
      prev: currentIndex > 0 ? chapterLinks[currentIndex - 1] : null,
      next: currentIndex < chapterLinks.length - 1 ? chapterLinks[currentIndex + 1] : null,
    };
  }

  /**
   * 生成目录 HTML
   * 
   * @param toc 目录结构
   * @returns HTML 字符串
   */
  generateTOCHtml(toc: TableOfContents): string {
    const lines: string[] = [];
    lines.push('<nav class="toc">');
    lines.push('<ul class="toc-chapters">');

    for (const chapter of toc.chapters) {
      lines.push(`  <li class="toc-chapter" data-chapter-id="${chapter.id}">`);
      lines.push(`    <a href="${this.generateChapterUrl(chapter.id)}" class="toc-chapter-link">`);
      lines.push(`      <span class="toc-chapter-title">${this.escapeHtml(chapter.title)}</span>`);
      lines.push(`      <span class="toc-chapter-count">${chapter.slideCount}</span>`);
      lines.push('    </a>');

      if (this.options.includeSlideLinks && chapter.slides.length > 0) {
        lines.push('    <ul class="toc-slides">');

        for (let i = 0; i < chapter.slides.length; i++) {
          const slide = chapter.slides[i];
          lines.push(`      <li class="toc-slide" data-slide-id="${slide.id}">`);
          lines.push(`        <a href="${this.generateSlideUrl(chapter.id, i)}" class="toc-slide-link">`);
          lines.push(`          <span class="toc-slide-title">${this.escapeHtml(slide.title)}</span>`);
          lines.push('        </a>');
          lines.push('      </li>');
        }

        lines.push('    </ul>');
      }

      lines.push('  </li>');
    }

    lines.push('</ul>');
    lines.push('</nav>');

    return lines.join('\n');
  }

  /**
   * 转义 HTML 特殊字符
   */
  private escapeHtml(text: string): string {
    const htmlEntities: Record<string, string> = {
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      '"': '&quot;',
      "'": '&#39;',
    };

    return text.replace(/[&<>"']/g, char => htmlEntities[char] || char);
  }
}

/**
 * 创建 TOCBuilder 实例的工厂函数
 * 
 * @param options 配置选项
 * @returns TOCBuilder 实例
 */
export function createTOCBuilder(options?: TOCBuilderOptions): TOCBuilder {
  return new TOCBuilder(options);
}

/**
 * 默认导出
 */
export default TOCBuilder;
