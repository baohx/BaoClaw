/**
 * SlideGenerator Unit Tests
 * 
 * 测试 SlideGenerator 模块的功能
 * 
 * Validates: Requirements 4.1
 */

import { describe, it, expect } from 'vitest';
import { SlideGenerator } from './slide';
import type { ParsedChapter, CodeBlock } from '../types';

// 创建测试用的章节数据
const createTestChapter = (
  id: string,
  title: string,
  order: number,
  options?: {
    sections?: Partial<ParsedChapter['sections']>;
    codeBlocks?: CodeBlock[];
  }
): ParsedChapter => ({
  id,
  order,
  title,
  sections: options?.sections ?? {
    problem: { title: '问题', content: '这是问题部分的内容', lineNumber: 1 },
    pattern: { title: '模式', content: '这是模式部分的内容', lineNumber: 10 },
    implementation: { title: '实现', content: '这是实现部分的内容', lineNumber: 20 },
    reflection: { title: '思考', content: '这是思考部分的内容', lineNumber: 30 },
    summary: { title: '总结', content: '这是总结部分的内容', lineNumber: 40 },
  },
  codeBlocks: options?.codeBlocks ?? [],
  assets: [],
  externalLinks: [],
});

describe('SlideGenerator', () => {
  describe('generateChapter', () => {
    it('should generate slides for a chapter', () => {
      const generator = new SlideGenerator();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const slides = generator.generateChapter(chapter);

      // 应该有标题幻灯片 + 各部分的幻灯片
      expect(slides.length).toBeGreaterThan(0);
      expect(slides[0].type).toBe('title');
      expect(slides[0].title).toBe('基础部分');
    });

    it('should generate title slide first', () => {
      const generator = new SlideGenerator();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const slides = generator.generateChapter(chapter);

      expect(slides[0].type).toBe('title');
      expect(slides[0].chapterId).toBe('01-fundamentals');
      expect(slides[0].chapterTitle).toBe('基础部分');
    });

    it('should generate slides for each section', () => {
      const generator = new SlideGenerator();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const slides = generator.generateChapter(chapter);
      const slideTypes = slides.map(s => s.type);

      // 应该包含各类型的幻灯片
      expect(slideTypes).toContain('problem');
      expect(slideTypes).toContain('pattern');
      expect(slideTypes).toContain('implementation');
      expect(slideTypes).toContain('reflection');
    });

    it('should generate unique slide IDs', () => {
      const generator = new SlideGenerator();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const slides = generator.generateChapter(chapter);
      const ids = slides.map(s => s.id);
      const uniqueIds = new Set(ids);

      expect(uniqueIds.size).toBe(ids.length);
    });

    it('should include chapter ID in slide ID', () => {
      const generator = new SlideGenerator();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const slides = generator.generateChapter(chapter);

      for (const slide of slides) {
        expect(slide.id).toContain('01-fundamentals');
      }
    });

    it('should handle chapters with code blocks', () => {
      const generator = new SlideGenerator();
      const codeBlocks: CodeBlock[] = [
        {
          id: 'code-1',
          language: 'rust',
          code: 'fn main() { println!("Hello"); }',
          sourcePath: 'src/main.rs',
          lineRange: { start: 1, end: 5 },
        },
      ];
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0, { codeBlocks });

      const slides = generator.generateChapter(chapter);
      const codeSlides = slides.filter(s => s.type === 'code');

      expect(codeSlides.length).toBeGreaterThan(0);
    });
  });

  describe('generateAll', () => {
    it('should generate slides for all chapters', () => {
      const generator = new SlideGenerator();
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const collection = generator.generateAll(chapters);

      expect(collection.slides.length).toBeGreaterThan(0);
      expect(collection.totalSlides).toBe(collection.slides.length);
    });

    it('should generate table of contents', () => {
      const generator = new SlideGenerator();
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const collection = generator.generateAll(chapters);

      expect(collection.tableOfContents).toBeDefined();
      expect(collection.tableOfContents.chapters.length).toBe(2);
    });

    it('should calculate progress for each slide', () => {
      const generator = new SlideGenerator();
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const collection = generator.generateAll(chapters);

      // 进度应该从 0 到接近 100
      const progressValues = collection.slides.map(s => s.progress);
      expect(Math.min(...progressValues)).toBe(0);
      expect(Math.max(...progressValues)).toBeGreaterThan(0);
      expect(Math.max(...progressValues)).toBeLessThanOrEqual(100);
    });

    it('should maintain chapter order in slides', () => {
      const generator = new SlideGenerator();
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
      ];

      const collection = generator.generateAll(chapters);

      // 第一个幻灯片应该是第一个章节的
      expect(collection.slides[0].chapterId).toBe('01-fundamentals');
      
      // 最后一个幻灯片应该是最后一个章节的
      const lastSlide = collection.slides[collection.slides.length - 1];
      expect(lastSlide.chapterId).toBe('02-core-implementation');
    });
  });

  describe('slide content generation', () => {
    it('should generate HTML content for slides', () => {
      const generator = new SlideGenerator();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const slides = generator.generateChapter(chapter);
      const problemSlide = slides.find(s => s.type === 'problem');

      expect(problemSlide).toBeDefined();
      expect(problemSlide!.content.length).toBeGreaterThan(0);
    });

    it('should escape HTML in code blocks', () => {
      const generator = new SlideGenerator();
      const codeBlocks: CodeBlock[] = [
        {
          id: 'code-1',
          language: 'rust',
          code: 'fn main() { let x = "<script>"; }',
        },
      ];
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0, { codeBlocks });

      const slides = generator.generateChapter(chapter);
      const codeSlide = slides.find(s => s.type === 'code');

      expect(codeSlide).toBeDefined();
      // 应该转义 < 和 > 字符
      expect(codeSlide!.content).toContain('&lt;');
      expect(codeSlide!.content).toContain('&gt;');
    });

    it('should include source path in code slides when available', () => {
      const generator = new SlideGenerator();
      const codeBlocks: CodeBlock[] = [
        {
          id: 'code-1',
          language: 'rust',
          code: 'fn main() {}',
          sourcePath: 'src/main.rs',
          lineRange: { start: 1, end: 5 },
        },
      ];
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0, { codeBlocks });

      const slides = generator.generateChapter(chapter);
      const codeSlide = slides.find(s => s.type === 'code');

      expect(codeSlide).toBeDefined();
      expect(codeSlide!.content).toContain('src/main.rs');
      expect(codeSlide!.content).toContain('lines 1-5');
    });
  });

  describe('table of contents generation', () => {
    it('should include all chapters in TOC', () => {
      const generator = new SlideGenerator();
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
        createTestChapter('02-core-implementation', '核心实现', 1),
        createTestChapter('03-memory-context', '记忆与上下文', 2),
      ];

      const collection = generator.generateAll(chapters);

      expect(collection.tableOfContents.chapters).toHaveLength(3);
      expect(collection.tableOfContents.chapters.map(c => c.id)).toEqual([
        '01-fundamentals',
        '02-core-implementation',
        '03-memory-context',
      ]);
    });

    it('should count slides per chapter correctly', () => {
      const generator = new SlideGenerator();
      const chapters: ParsedChapter[] = [
        createTestChapter('01-fundamentals', '基础部分', 0),
      ];

      const collection = generator.generateAll(chapters);
      const chapterSlides = collection.slides.filter(s => s.chapterId === '01-fundamentals');

      expect(collection.tableOfContents.chapters[0].slideCount).toBe(chapterSlides.length);
    });

    it('should list section names in TOC', () => {
      const generator = new SlideGenerator();
      const chapter = createTestChapter('01-fundamentals', '基础部分', 0);

      const collection = generator.generateAll([chapter]);

      const sections = collection.tableOfContents.chapters[0].sections;
      expect(sections).toContain('问题');
      expect(sections).toContain('模式');
      expect(sections).toContain('实现');
      expect(sections).toContain('思考');
    });
  });

  describe('edge cases', () => {
    it('should handle empty chapters array', () => {
      const generator = new SlideGenerator();
      const collection = generator.generateAll([]);

      expect(collection.slides).toEqual([]);
      expect(collection.tableOfContents.chapters).toEqual([]);
      expect(collection.totalSlides).toBe(0);
    });

    it('should handle chapter with missing sections', () => {
      const generator = new SlideGenerator();
      const chapter: ParsedChapter = {
        id: '01-fundamentals',
        order: 0,
        title: '基础部分',
        sections: {
          problem: { title: '问题', content: '内容', lineNumber: 1 },
          // 缺少其他部分
        },
        codeBlocks: [],
        assets: [],
        externalLinks: [],
      };

      const slides = generator.generateChapter(chapter);

      // 至少应该有标题幻灯片和问题幻灯片
      expect(slides.length).toBeGreaterThanOrEqual(2);
      expect(slides[0].type).toBe('title');
    });

    it('should handle special characters in chapter title', () => {
      const generator = new SlideGenerator();
      const chapter = createTestChapter('01-test', '<Script>alert("XSS")</Script>', 0);

      const slides = generator.generateChapter(chapter);

      // 内容应该转义 HTML 特殊字符
      expect(slides[0].content).not.toContain('<Script>');
      expect(slides[0].content).toContain('&lt;');
    });
  });
});
