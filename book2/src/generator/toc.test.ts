/**
 * TOCBuilder Unit Tests
 * 
 * 测试 TOCBuilder 模块的功能
 */

import { describe, it, expect } from 'vitest';
import { TOCBuilder, createTOCBuilder } from './toc';
import type { ParsedChapter } from '../types';

// 创建测试用的章节数据
const createTestChapter = (
  id: string,
  title: string,
  order: number,
  sections?: Partial<ParsedChapter['sections']>
): ParsedChapter => ({
  id,
  order,
  title,
  sections: {
    problem: { title: '问题', content: '这是问题部分', lineNumber: 1 },
    pattern: { title: '模式', content: '这是模式部分', lineNumber: 10 },
    implementation: { title: '实现', content: '这是实现部分', lineNumber: 20 },
    reflection: { title: '思考', content: '这是思考部分', lineNumber: 30 },
    ...sections,
  },
  codeBlocks: [],
  assets: [],
  externalLinks: [],
});

describe('TOCBuilder', () => {

  describe('build', () => {
    it('should build table of contents from parsed chapters', () => {
      const builder = new TOCBuilder();
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const toc = builder.build(chapters);

      expect(toc.chapters).toHaveLength(2);
      expect(toc.chapters[0].id).toBe('01-fundamentals');
      expect(toc.chapters[0].title).toBe('基础部分');
      expect(toc.chapters[1].id).toBe('02-core-implementation');
      expect(toc.chapters[1].title).toBe('核心实现');
    });

    it('should sort chapters by order when sortByOrder is true', () => {
      const builder = new TOCBuilder({ sortByOrder: true });
      const chapters: ParsedChapter[] = [
        createTestChapter('02-core-implementation', '核心实现', 1),
        createTestChapter('01-fundamentals', '基础部分', 0),
      ];

      const toc = builder.build(chapters);

      expect(toc.chapters[0].id).toBe('01-fundamentals');
      expect(toc.chapters[1].id).toBe('02-core-implementation');
    });

    it('should not sort chapters when sortByOrder is false', () => {
      const builder = new TOCBuilder({ sortByOrder: false });
      const chapters: ParsedChapter[] = [
        createTestChapter('02-core-implementation', '核心实现', 1),
        createTestChapter('01-fundamentals', '基础部分', 0),
      ];

      const toc = builder.build(chapters);

      expect(toc.chapters[0].id).toBe('02-core-implementation');
      expect(toc.chapters[1].id).toBe('01-fundamentals');
    });

    it('should extract section names from chapters', () => {
      const builder = new TOCBuilder();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0, {
        summary: { title: '总结', content: '这是总结部分', lineNumber: 40 },
      });

      const toc = builder.build([chapter]);

      expect(toc.chapters[0].sections).toContain('问题');
      expect(toc.chapters[0].sections).toContain('模式');
      expect(toc.chapters[0].sections).toContain('实现');
      expect(toc.chapters[0].sections).toContain('思考');
      expect(toc.chapters[0].sections).toContain('总结');
    });

    it('should generate slide entries for each chapter', () => {
      const builder = new TOCBuilder();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const toc = builder.build([chapter]);

      // 至少应该有标题页 + 4个部分页
      expect(toc.chapters[0].slideCount).toBeGreaterThanOrEqual(5);
      expect(toc.chapters[0].slides[0].type).toBe('title');
    });

    it('should handle chapters with missing optional sections', () => {
      const builder = new TOCBuilder();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0, {
        summary: undefined,
      });

      const toc = builder.build([chapter]);

      expect(toc.chapters[0].sections).not.toContain('总结');
    });
  });

  describe('generateNavigationLinks', () => {
    it('should generate navigation links for chapters', () => {
      const builder = new TOCBuilder({ includeSlideLinks: false });
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const links = builder.generateNavigationLinks(chapters);

      expect(links).toHaveLength(2);
      expect(links[0].isChapter).toBe(true);
      expect(links[0].url).toBe('#/01-fundamentals');
      expect(links[1].url).toBe('#/02-core-implementation');
    });

    it('should generate navigation links for slides when includeSlideLinks is true', () => {
      const builder = new TOCBuilder({ includeSlideLinks: true });
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const links = builder.generateNavigationLinks([chapter]);

      // 应该有章节链接 + 多个幻灯片链接
      const chapterLinks = links.filter(l => l.isChapter);
      const slideLinks = links.filter(l => !l.isChapter);

      expect(chapterLinks).toHaveLength(1);
      expect(slideLinks.length).toBeGreaterThan(0);
    });

    it('should use custom URL prefix', () => {
      const builder = new TOCBuilder({ urlPrefix: '/slides/', includeSlideLinks: false });
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
      ];

      const links = builder.generateNavigationLinks(chapters);

      expect(links[0].url).toBe('/slides/01-fundamentals');
    });
  });

  describe('parseSlideUrl', () => {
    it('should parse chapter URL correctly', () => {
      const builder = new TOCBuilder();

      const result = builder.parseSlideUrl('#/01-fundamentals');

      expect(result).not.toBeNull();
      expect(result?.chapterId).toBe('01-fundamentals');
      expect(result?.slideIndex).toBe(0);
    });

    it('should parse slide URL correctly', () => {
      const builder = new TOCBuilder();

      const result = builder.parseSlideUrl('#/01-fundamentals/03');

      expect(result).not.toBeNull();
      expect(result?.chapterId).toBe('01-fundamentals');
      expect(result?.slideIndex).toBe(3);
    });

    it('should return null for invalid URL', () => {
      const builder = new TOCBuilder();

      expect(builder.parseSlideUrl('')).toBeNull();
      expect(builder.parseSlideUrl('#/')).toBeNull();
    });

    it('should handle custom URL prefix', () => {
      const builder = new TOCBuilder({ urlPrefix: '/slides/' });

      const result = builder.parseSlideUrl('/slides/01-fundamentals/02');

      expect(result).not.toBeNull();
      expect(result?.chapterId).toBe('01-fundamentals');
      expect(result?.slideIndex).toBe(2);
    });
  });

  describe('getAdjacentChapters', () => {
    it('should return prev and next chapters', () => {
      const builder = new TOCBuilder({ includeSlideLinks: false });
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
        createTestChapter('03-memory-context', '记忆与上下文', 2),
      ];

      const links = builder.generateNavigationLinks(chapters);
      const adjacent = builder.getAdjacentChapters(links, '02-core-implementation');

      expect(adjacent.prev).not.toBeNull();
      expect(adjacent.prev?.id).toBe('01-fundamentals');
      expect(adjacent.next).not.toBeNull();
      expect(adjacent.next?.id).toBe('03-memory-context');
    });

    it('should return null for prev when at first chapter', () => {
      const builder = new TOCBuilder({ includeSlideLinks: false });
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const links = builder.generateNavigationLinks(chapters);
      const adjacent = builder.getAdjacentChapters(links, '01-fundamentals');

      expect(adjacent.prev).toBeNull();
      expect(adjacent.next).not.toBeNull();
    });

    it('should return null for next when at last chapter', () => {
      const builder = new TOCBuilder({ includeSlideLinks: false });
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const links = builder.generateNavigationLinks(chapters);
      const adjacent = builder.getAdjacentChapters(links, '02-core-implementation');

      expect(adjacent.prev).not.toBeNull();
      expect(adjacent.next).toBeNull();
    });

    it('should return nulls for non-existent chapter', () => {
      const builder = new TOCBuilder({ includeSlideLinks: false });
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
      ];

      const links = builder.generateNavigationLinks(chapters);
      const adjacent = builder.getAdjacentChapters(links, 'non-existent');

      expect(adjacent.prev).toBeNull();
      expect(adjacent.next).toBeNull();
    });
  });

  describe('generateTOCHtml', () => {
    it('should generate valid HTML structure', () => {
      const builder = new TOCBuilder({ includeSlideLinks: false });
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
      ];

      const toc = builder.build(chapters);
      const html = builder.generateTOCHtml(toc);

      expect(html).toContain('<nav class="toc">');
      expect(html).toContain('<ul class="toc-chapters">');
      expect(html).toContain('01-fundamentals');
      expect(html).toContain('基础部分');
    });

    it('should escape HTML in titles', () => {
      const builder = new TOCBuilder();
      const chapter: ParsedChapter = {
        id: '01-test',
        order: 0,
        title: '<script>alert("XSS")</script>',
        sections: {},
        codeBlocks: [],
        assets: [],
        externalLinks: [],
      };

      const toc = builder.build([chapter]);
      const html = builder.generateTOCHtml(toc);

      expect(html).not.toContain('<script>');
      expect(html).toContain('&lt;script&gt;');
    });

    it('should include slide links when configured', () => {
      const builder = new TOCBuilder({ includeSlideLinks: true });
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const toc = builder.build([chapter]);
      const html = builder.generateTOCHtml(toc);

      expect(html).toContain('<ul class="toc-slides">');
    });
  });

  describe('findChapterSlideLinks', () => {
    it('should find all slide links for a chapter', () => {
      const builder = new TOCBuilder({ includeSlideLinks: true });
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const links = builder.generateNavigationLinks(chapters);
      const slideLinks = builder.findChapterSlideLinks(links, '01-fundamentals');

      expect(slideLinks.length).toBeGreaterThan(0);
      for (const link of slideLinks) {
        expect(link.id.startsWith('01-fundamentals')).toBe(true);
        expect(link.isChapter).toBe(false);
      }
    });
  });
});

describe('createTOCBuilder', () => {
  it('should create TOCBuilder instance with default options', () => {
    const builder = createTOCBuilder();

    expect(builder).toBeInstanceOf(TOCBuilder);
  });

  it('should create TOCBuilder instance with custom options', () => {
    const builder = createTOCBuilder({
      urlPrefix: '/custom/',
      includeSlideLinks: false,
    });

    expect(builder).toBeInstanceOf(TOCBuilder);
  });
});

describe('Property: Sidebar Contains All Chapters', () => {
  /**
   * Property 9: Sidebar Contains All Chapters
   * 
   * For any rendered table of contents sidebar, it SHALL contain entries
   * for all chapters in the book, and each entry SHALL link to the correct chapter.
   * 
   * Validates: Requirements 4.4
   */
  it('should contain all chapters in the generated TOC', () => {
    const builder = new TOCBuilder();
    const chapters: ParsedChapter[] = [
      createTestChapter('01-fundamentals', '基础部分', 0),
      createTestChapter('02-core-implementation', '核心实现', 1),
      createTestChapter('03-memory-context', '记忆与上下文', 2),
      createTestChapter('04-ipc-multiclient', 'IPC 与多客户端', 3),
      createTestChapter('05-production', '生产实践', 4),
      createTestChapter('06-advanced-patterns', '高级模式', 5),
    ];

    const toc = builder.build(chapters);

    // 验证所有章节都在目录中
    expect(toc.chapters).toHaveLength(6);
    
    const chapterIds = toc.chapters.map(c => c.id);
    expect(chapterIds).toContain('01-fundamentals');
    expect(chapterIds).toContain('02-core-implementation');
    expect(chapterIds).toContain('03-memory-context');
    expect(chapterIds).toContain('04-ipc-multiclient');
    expect(chapterIds).toContain('05-production');
    expect(chapterIds).toContain('06-advanced-patterns');
  });

  it('should link to correct chapters via navigation links', () => {
    const builder = new TOCBuilder({ includeSlideLinks: false });
    const chapters: ParsedChapter[] = [
      createTestChapter('01-fundamentals', '基础部分', 0),
      createTestChapter('02-core-implementation', '核心实现', 1),
    ];

    const links = builder.generateNavigationLinks(chapters);

    // 验证每个章节都有正确的链接
    expect(links[0].id).toBe('01-fundamentals');
    expect(links[0].url).toBe('#/01-fundamentals');
    expect(links[1].id).toBe('02-core-implementation');
    expect(links[1].url).toBe('#/02-core-implementation');
  });
});
