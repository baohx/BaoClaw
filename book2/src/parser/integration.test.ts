/**
 * Integration Tests for Markdown Parser
 * 
 * 测试解析器与实际章节文件的集成
 */

import { describe, it, expect } from 'vitest';
import { MarkdownParser } from './markdown';
import { CodeExtractor } from './code-extractor';
import { join } from 'path';

describe('MarkdownParser Integration', () => {
  const parser = new MarkdownParser();
  const chaptersDir = join(__dirname, '../../chapters');

  describe('parse sample chapter', () => {
    it('should parse the fundamentals chapter correctly', async () => {
      const chapterPath = join(chaptersDir, '01-fundamentals', 'README.md');
      
      const result = await parser.parseFile(chapterPath);
      
      expect(result.id).toBe('01-fundamentals');
      expect(result.title).toBe('Agent 基础');
      expect(result.order).toBe(0);
    });

    it('should extract all required sections from the sample', async () => {
      const chapterPath = join(chaptersDir, '01-fundamentals', 'README.md');
      
      const result = await parser.parseFile(chapterPath);
      
      expect(result.sections.problem).toBeDefined();
      expect(result.sections.pattern).toBeDefined();
      expect(result.sections.implementation).toBeDefined();
      expect(result.sections.reflection).toBeDefined();
      expect(result.sections.summary).toBeDefined();
    });

    it('should extract code blocks with source paths', async () => {
      const chapterPath = join(chaptersDir, '01-fundamentals', 'README.md');
      
      const result = await parser.parseFile(chapterPath);
      
      // Should have multiple code blocks
      expect(result.codeBlocks.length).toBeGreaterThan(0);
      
      // At least one Rust code block with source path
      const rustWithPath = result.codeBlocks.find(
        b => b.language === 'rust' && b.sourcePath
      );
      expect(rustWithPath).toBeDefined();
      expect(rustWithPath?.sourcePath).toBe('baoclaw-core/src/engine/query_engine.rs');
      
      // At least one TypeScript code block with source path
      const tsWithPath = result.codeBlocks.find(
        b => b.language === 'typescript' && b.sourcePath
      );
      expect(tsWithPath).toBeDefined();
      expect(tsWithPath?.sourcePath).toBe('ts-ipc/cli.ts');
    });

    it('should extract mermaid diagrams', async () => {
      const chapterPath = join(chaptersDir, '01-fundamentals', 'README.md');
      
      const result = await parser.parseFile(chapterPath);
      
      const mermaidBlock = result.codeBlocks.find(b => b.language === 'mermaid');
      expect(mermaidBlock).toBeDefined();
      expect(mermaidBlock?.code).toContain('graph LR');
    });

    it('should extract external links', async () => {
      const chapterPath = join(chaptersDir, '01-fundamentals', 'README.md');
      
      const result = await parser.parseFile(chapterPath);
      
      expect(result.externalLinks.length).toBeGreaterThan(0);
      
      // Should have GitHub link
      const githubLink = result.externalLinks.find(
        l => l.url.includes('github.com')
      );
      expect(githubLink).toBeDefined();
      expect(githubLink?.type).toBe('github');
    });

    it('should parse content with CodeExtractor', async () => {
      const chapterPath = join(chaptersDir, '01-fundamentals', 'README.md');
      const { readFile } = await import('fs/promises');
      const content = await readFile(chapterPath, 'utf-8');
      
      const extractor = new CodeExtractor();
      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks.length).toBeGreaterThan(0);
      
      // Check statistics
      const stats = extractor.getStatistics(content);
      expect(stats.total).toBeGreaterThan(0);
      expect(stats.byLanguage.rust).toBeGreaterThan(0);
      expect(stats.byLanguage.typescript).toBeGreaterThan(0);
      expect(stats.withSourcePath).toBeGreaterThan(0);
    });
  });
});
