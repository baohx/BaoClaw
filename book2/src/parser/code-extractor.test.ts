/**
 * Code Extractor Unit Tests
 * 
 * 测试代码块提取和元数据解析功能
 * Requirements: 3.1, 3.2
 */

import { describe, it, expect } from 'vitest';
import { CodeExtractor } from './code-extractor';
import type { CodeBlock } from '../types';

describe('CodeExtractor', () => {
  const extractor = new CodeExtractor();

  describe('extractCodeBlocks', () => {
    it('should extract a simple Rust code block', () => {
      const content = `
# Chapter Title

Here is some Rust code:

\`\`\`rust
fn main() {
    println!("Hello, World!");
}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].language).toBe('rust');
      expect(blocks[0].code).toContain('fn main()');
      expect(blocks[0].id).toBe('code-block-0');
    });

    it('should extract a TypeScript code block', () => {
      const content = `
\`\`\`typescript
interface User {
  name: string;
  age: number;
}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].language).toBe('typescript');
      expect(blocks[0].code).toContain('interface User');
    });

    it('should extract a bash code block', () => {
      const content = `
\`\`\`bash
cargo build --release
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].language).toBe('bash');
    });

    it('should extract a mermaid diagram', () => {
      const content = `
\`\`\`mermaid
graph TD
    A --> B
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].language).toBe('mermaid');
    });

    it('should extract multiple code blocks', () => {
      const content = `
\`\`\`rust
fn first() {}
\`\`\`

Some text between.

\`\`\`typescript
function second() {}
\`\`\`

\`\`\`bash
echo "third"
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(3);
      expect(blocks[0].language).toBe('rust');
      expect(blocks[1].language).toBe('typescript');
      expect(blocks[2].language).toBe('bash');
    });

    it('should handle code blocks without language', () => {
      const content = `
\`\`\`
plain text code
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].language).toBe('other');
    });

    it('should return empty array for no code blocks', () => {
      const content = `
# Just a heading

Some paragraph text.

- List item 1
- List item 2
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(0);
    });
  });

  describe('parseMetadata - path attribute', () => {
    it('should parse path attribute', () => {
      const content = `
\`\`\`rust path="baoclaw-core/src/engine/query.rs"
fn query() {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].sourcePath).toBe('baoclaw-core/src/engine/query.rs');
    });

    it('should handle paths with subdirectories', () => {
      const content = `
\`\`\`rust path="baoclaw-core/src/engine/executor.rs"
fn execute() {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks[0].sourcePath).toBe('baoclaw-core/src/engine/executor.rs');
    });
  });

  describe('parseMetadata - lines attribute', () => {
    it('should parse lines attribute', () => {
      const content = `
\`\`\`rust lines="45-78"
fn important_function() {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].lineRange).toEqual({ start: 45, end: 78 });
    });

    it('should parse single-digit line ranges', () => {
      const content = `
\`\`\`rust lines="1-5"
fn small() {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks[0].lineRange).toEqual({ start: 1, end: 5 });
    });

    it('should parse multi-digit line ranges', () => {
      const content = `
\`\`\`rust lines="100-250"
fn large_function() {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks[0].lineRange).toEqual({ start: 100, end: 250 });
    });
  });

  describe('parseMetadata - combined attributes', () => {
    it('should parse both path and lines attributes', () => {
      const content = `
\`\`\`rust path="baoclaw-core/src/engine/query.rs" lines="45-78"
fn query_engine() {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].sourcePath).toBe('baoclaw-core/src/engine/query.rs');
      expect(blocks[0].lineRange).toEqual({ start: 45, end: 78 });
    });

    it('should parse attributes in reverse order', () => {
      const content = `
\`\`\`rust lines="10-20" path="src/main.rs"
fn main() {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks[0].sourcePath).toBe('src/main.rs');
      expect(blocks[0].lineRange).toEqual({ start: 10, end: 20 });
    });
  });

  describe('validateMetadata', () => {
    it('should validate a correct path', () => {
      const codeBlock: CodeBlock = {
        id: 'test',
        language: 'rust',
        code: 'fn test() {}',
        sourcePath: 'baoclaw-core/src/main.rs',
      };

      const result = extractor.validateMetadata(codeBlock);
      
      expect(result.valid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    it('should reject absolute paths', () => {
      const codeBlock: CodeBlock = {
        id: 'test',
        language: 'rust',
        code: 'fn test() {}',
        sourcePath: '/absolute/path/to/file.rs',
      };

      const result = extractor.validateMetadata(codeBlock);
      
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Source path should be relative, got: /absolute/path/to/file.rs');
    });

    it('should reject paths without valid extensions', () => {
      const codeBlock: CodeBlock = {
        id: 'test',
        language: 'rust',
        code: 'fn test() {}',
        sourcePath: 'src/file.txt',
      };

      const result = extractor.validateMetadata(codeBlock);
      
      expect(result.valid).toBe(false);
      expect(result.errors.some(e => e.includes('valid extension'))).toBe(true);
    });

    it('should validate valid line ranges', () => {
      const codeBlock: CodeBlock = {
        id: 'test',
        language: 'rust',
        code: 'fn test() {}',
        lineRange: { start: 1, end: 10 },
      };

      const result = extractor.validateMetadata(codeBlock);
      
      expect(result.valid).toBe(true);
    });

    it('should reject line range start < 1', () => {
      const codeBlock: CodeBlock = {
        id: 'test',
        language: 'rust',
        code: 'fn test() {}',
        lineRange: { start: 0, end: 10 },
      };

      const result = extractor.validateMetadata(codeBlock);
      
      expect(result.valid).toBe(false);
      expect(result.errors.some(e => e.includes('must be >= 1'))).toBe(true);
    });

    it('should reject end < start in line range', () => {
      const codeBlock: CodeBlock = {
        id: 'test',
        language: 'rust',
        code: 'fn test() {}',
        lineRange: { start: 20, end: 10 },
      };

      const result = extractor.validateMetadata(codeBlock);
      
      expect(result.valid).toBe(false);
      expect(result.errors.some(e => e.includes('must be >= start'))).toBe(true);
    });
  });

  describe('extractByLanguage', () => {
    it('should filter by Rust language', () => {
      const content = `
\`\`\`rust
fn rust_fn() {}
\`\`\`

\`\`\`typescript
function ts_fn() {}
\`\`\`

\`\`\`rust
fn another_rust_fn() {}
\`\`\`
`;

      const rustBlocks = extractor.extractByLanguage(content, 'rust');
      
      expect(rustBlocks).toHaveLength(2);
      expect(rustBlocks.every(b => b.language === 'rust')).toBe(true);
    });

    it('should return empty array for no matching language', () => {
      const content = `
\`\`\`rust
fn rust_fn() {}
\`\`\`
`;

      const tsBlocks = extractor.extractByLanguage(content, 'typescript');
      
      expect(tsBlocks).toHaveLength(0);
    });
  });

  describe('extractWithSourcePath', () => {
    it('should extract only blocks with source paths', () => {
      const content = `
\`\`\`rust path="src/main.rs"
fn main() {}
\`\`\`

\`\`\`rust
fn no_path() {}
\`\`\`

\`\`\`typescript path="src/index.ts"
function index() {}
\`\`\`
`;

      const blocksWithPath = extractor.extractWithSourcePath(content);
      
      expect(blocksWithPath).toHaveLength(2);
      expect(blocksWithPath.every(b => b.sourcePath !== undefined)).toBe(true);
    });
  });

  describe('getStatistics', () => {
    it('should return correct statistics', () => {
      const content = `
\`\`\`rust path="src/main.rs" lines="1-10"
fn main() {}
\`\`\`

\`\`\`rust
fn no_meta() {}
\`\`\`

\`\`\`typescript path="src/index.ts"
function index() {}
\`\`\`

\`\`\`bash
echo "hello"
\`\`\`
`;

      const stats = extractor.getStatistics(content);
      
      expect(stats.total).toBe(4);
      expect(stats.byLanguage.rust).toBe(2);
      expect(stats.byLanguage.typescript).toBe(1);
      expect(stats.byLanguage.bash).toBe(1);
      expect(stats.byLanguage.mermaid).toBe(0);
      expect(stats.byLanguage.other).toBe(0);
      expect(stats.withSourcePath).toBe(2);
      expect(stats.withLineRange).toBe(1);
    });
  });

  describe('resolveSourcePath', () => {
    it('should resolve source path relative to baoclaw root', () => {
      const codeBlock: CodeBlock = {
        id: 'test',
        language: 'rust',
        code: 'fn test() {}',
        sourcePath: 'baoclaw-core/src/main.rs',
      };

      const resolved = extractor.resolveSourcePath(codeBlock, '/projects/BaoClaw');
      
      expect(resolved).toBe('/projects/BaoClaw/baoclaw-core/src/main.rs');
    });

    it('should return null for code block without source path', () => {
      const codeBlock: CodeBlock = {
        id: 'test',
        language: 'rust',
        code: 'fn test() {}',
      };

      const resolved = extractor.resolveSourcePath(codeBlock, '/projects/BaoClaw');
      
      expect(resolved).toBeNull();
    });
  });

  describe('language normalization', () => {
    it('should normalize "rs" to "rust"', () => {
      const content = `
\`\`\`rs
fn main() {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks[0].language).toBe('rust');
    });

    it('should normalize "ts" to "typescript"', () => {
      const content = `
\`\`\`ts
interface User {}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks[0].language).toBe('typescript');
    });

    it('should normalize "sh" to "bash"', () => {
      const content = `
\`\`\`sh
echo "hello"
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks[0].language).toBe('bash');
    });

    it('should normalize "shell" to "bash"', () => {
      const content = `
\`\`\`shell
echo "hello"
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks[0].language).toBe('bash');
    });
  });

  describe('edge cases', () => {
    it('should handle empty code blocks', () => {
      const content = `
\`\`\`rust
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].code).toBe('');
    });

    it('should handle code blocks with only whitespace', () => {
      const content = `
\`\`\`rust
   
   
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].code).toBe('');
    });

    it('should handle complex code with special characters', () => {
      const content = `
\`\`\`rust
fn complex() -> Result<(), Box<dyn std::error::Error>> {
    let regex = Regex::new(r"\\d+")?;
    Ok(())
}
\`\`\`
`;

      const blocks = extractor.extractCodeBlocks(content);
      
      expect(blocks).toHaveLength(1);
      expect(blocks[0].code).toContain('Regex::new');
    });

    it('should handle code blocks containing triple backticks in strings', () => {
      const content = `
\`\`\`rust
let markdown = r"\`\`\`code\`\`\`";
\`\`\`
`;

      // This should not break the parser
      // Note: In real markdown, this would need escaping, but we test robustness
      const blocks = extractor.extractCodeBlocks(content);
      
      // Should extract at least the first code block correctly
      expect(blocks.length).toBeGreaterThanOrEqual(1);
    });
  });
});
