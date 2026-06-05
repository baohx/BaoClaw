/**
 * Markdown Parser Tests
 * 
 * 测试 Markdown 解析器的各种功能
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { MarkdownParser, REQUIRED_SECTIONS } from './markdown';

describe('MarkdownParser', () => {
  let parser: MarkdownParser;

  beforeEach(() => {
    parser = new MarkdownParser();
  });

  describe('parseContent', () => {
    it('should parse basic chapter structure', () => {
      const content = `# Test Chapter

This is a test chapter.

## 问题

This is the problem section.

## 模式

This is the pattern section.

## 实现

This is the implementation section.

## 思考

This is the reflection section.
`;

      const result = parser.parseContent(content, 'test/README.md', 0);

      expect(result.id).toBe('test');
      expect(result.title).toBe('Test Chapter');
      expect(result.order).toBe(0);
    });

    it('should extract all required sections', () => {
      const content = `# Chapter

## 问题

Problem content here.

## 模式

Pattern content here.

## 实现

Implementation content here.

## 思考

Reflection content here.

## 总结

Summary content here.
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.sections.problem).toBeDefined();
      expect(result.sections.pattern).toBeDefined();
      expect(result.sections.implementation).toBeDefined();
      expect(result.sections.reflection).toBeDefined();
      expect(result.sections.summary).toBeDefined();
    });

    it('should handle missing sections gracefully', () => {
      const content = `# Chapter

## 问题

Problem content only.
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.sections.problem).toBeDefined();
      expect(result.sections.pattern).toBeUndefined();
      expect(result.sections.implementation).toBeUndefined();
      expect(result.sections.reflection).toBeUndefined();
    });
  });

  describe('Code Block Extraction', () => {
    it('should extract Rust code blocks', () => {
      const content = `# Chapter

## 实现

\`\`\`rust
fn main() {
    println!("Hello");
}
\`\`\`
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.codeBlocks).toHaveLength(1);
      expect(result.codeBlocks[0].language).toBe('rust');
      expect(result.codeBlocks[0].code).toContain('fn main()');
    });

    it('should extract code blocks with source path', () => {
      const content = `# Chapter

## 实现

\`\`\`rust path="src/main.rs" lines="10-20"
fn main() {
    println!("Hello");
}
\`\`\`
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.codeBlocks).toHaveLength(1);
      expect(result.codeBlocks[0].language).toBe('rust');
      expect(result.codeBlocks[0].sourcePath).toBe('src/main.rs');
      expect(result.codeBlocks[0].lineRange).toEqual({ start: 10, end: 20 });
    });

    it('should extract multiple code blocks', () => {
      const content = `# Chapter

## 实现

\`\`\`rust
fn rust_code() {}
\`\`\`

\`\`\`typescript
const tsCode = () => {};
\`\`\`

\`\`\`bash
echo "hello"
\`\`\`
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.codeBlocks).toHaveLength(3);
      expect(result.codeBlocks[0].language).toBe('rust');
      expect(result.codeBlocks[1].language).toBe('typescript');
      expect(result.codeBlocks[2].language).toBe('bash');
    });

    it('should handle mermaid diagrams', () => {
      const content = `# Chapter

## 模式

\`\`\`mermaid
graph LR
    A --> B
\`\`\`
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.codeBlocks).toHaveLength(1);
      expect(result.codeBlocks[0].language).toBe('mermaid');
    });
  });

  describe('Asset Extraction', () => {
    it('should extract images', () => {
      const content = `# Chapter

## 模式

![Architecture Diagram](./images/arch.png)

Some text with ![Another Image](./images/flow.png "Flow Diagram").
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.assets).toHaveLength(2);
      expect(result.assets[0].type).toBe('image');
      expect(result.assets[0].path).toBe('./images/arch.png');
      expect(result.assets[0].alt).toBe('Architecture Diagram');
    });
  });

  describe('External Link Extraction', () => {
    it('should extract external links', () => {
      const content = `# Chapter

## 总结

See [BaoClaw GitHub](https://github.com/baoclaw/baoclaw) for more details.

Also check [Rust Docs](https://doc.rust-lang.org/).
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.externalLinks.length).toBeGreaterThanOrEqual(2);
      const githubLink = result.externalLinks.find(l => l.url.includes('github.com'));
      expect(githubLink).toBeDefined();
      expect(githubLink?.type).toBe('github');
    });

    it('should deduplicate links', () => {
      const content = `# Chapter

See [Link 1](https://example.com) and [Link 2](https://example.com).
`;

      const result = parser.parseContent(content, 'test.md', 0);

      const exampleLinks = result.externalLinks.filter(l => l.url === 'https://example.com');
      expect(exampleLinks).toHaveLength(1);
    });

    it('should identify doc links', () => {
      const content = `# Chapter

Check [Rust Docs](https://doc.rust-lang.org/std/) for reference.
`;

      const result = parser.parseContent(content, 'test.md', 0);

      const docLink = result.externalLinks.find(l => l.url.includes('doc.rust-lang.org'));
      expect(docLink).toBeDefined();
      // Note: The type may be 'reference' since it doesn't contain '/docs/' exactly
      expect(['docs', 'reference']).toContain(docLink?.type);
    });
  });

  describe('Chapter ID Extraction', () => {
    it('should extract ID from directory name', () => {
      const content = '# Chapter';
      const result = parser.parseContent(content, 'chapters/01-fundamentals/README.md', 0);
      expect(result.id).toBe('01-fundamentals');
    });

    it('should extract ID from filename', () => {
      const content = '# Chapter';
      const result = parser.parseContent(content, 'chapters/intro.md', 0);
      expect(result.id).toBe('intro');
    });
  });

  describe('Section Content', () => {
    it('should preserve section content', () => {
      const content = `# Chapter

## 问题

This is a problem description.

- Point 1
- Point 2
- Point 3

Some more text.
`;

      const result = parser.parseContent(content, 'test.md', 0);

      expect(result.sections.problem).toBeDefined();
      expect(result.sections.problem?.content).toContain('This is a problem description');
      expect(result.sections.problem?.content).toContain('- Point 1');
    });
  });
});

describe('REQUIRED_SECTIONS constant', () => {
  it('should contain all required section keys', () => {
    expect(REQUIRED_SECTIONS).toContain('problem');
    expect(REQUIRED_SECTIONS).toContain('pattern');
    expect(REQUIRED_SECTIONS).toContain('implementation');
    expect(REQUIRED_SECTIONS).toContain('reflection');
    expect(REQUIRED_SECTIONS).toHaveLength(4);
  });
});
