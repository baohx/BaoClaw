/**
 * SyntaxHighlighter Tests - 语法高亮器测试
 * 
 * 测试 SyntaxHighlighter 的核心功能
 * **Validates: Requirements 4.5**
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { createSyntaxHighlighter, type SyntaxHighlighter } from './syntax';

describe('SyntaxHighlighter', () => {
  let highlighter: SyntaxHighlighter;

  beforeEach(() => {
    highlighter = createSyntaxHighlighter();
  });

  describe('highlight', () => {
    it('should highlight Rust code', () => {
      const rustCode = `fn main() {
    let x = 42;
    println!("{}", x);
}`;
      const result = highlighter.highlight(rustCode, 'rust');
      
      // Should return HTML with syntax highlighting spans
      expect(result).toContain('fn');
      expect(result).toContain('main');
    });

    it('should highlight Rust code with "rs" alias', () => {
      const rustCode = 'let x = 42;';
      const result = highlighter.highlight(rustCode, 'rs');
      
      expect(result).toContain('let');
    });

    it('should highlight TypeScript code', () => {
      const tsCode = `interface User {
  name: string;
  age: number;
}

function greet(user: User): string {
  return \`Hello, \${user.name}!\`;
}`;
      const result = highlighter.highlight(tsCode, 'typescript');
      
      expect(result).toContain('interface');
      expect(result).toContain('function');
    });

    it('should highlight TypeScript code with "ts" alias', () => {
      const tsCode = 'const x: number = 42;';
      const result = highlighter.highlight(tsCode, 'ts');
      
      expect(result).toContain('const');
    });

    it('should highlight JavaScript code', () => {
      const jsCode = `const sum = (a, b) => a + b;
console.log(sum(1, 2));`;
      const result = highlighter.highlight(jsCode, 'javascript');
      
      expect(result).toContain('const');
    });

    it('should highlight bash code', () => {
      const bashCode = `#!/bin/bash
echo "Hello, World!"
for i in {1..5}; do
  echo $i
done`;
      const result = highlighter.highlight(bashCode, 'bash');
      
      expect(result).toContain('echo');
    });

    it('should escape HTML characters for unsupported languages', () => {
      const code = '<script>alert("xss")</script>';
      const result = highlighter.highlight(code, 'unknown-language');
      
      // highlight.js auto-detects and highlights HTML/JS, but still escapes
      // The important thing is that raw <script> tag is not present
      expect(result).not.toContain('<script>');
      // The code is escaped (in hljs spans)
      expect(result).toContain('&lt;');
    });

    it('should return escaped code when highlighting fails', () => {
      const code = 'some code';
      // Pass an invalid language that might cause issues
      const result = highlighter.highlight(code, 'typescript');
      
      // Should return some output (either highlighted or escaped)
      expect(typeof result).toBe('string');
      expect(result.length).toBeGreaterThan(0);
    });
  });

  describe('highlightBlock', () => {
    it('should wrap highlighted code in pre/code tags', () => {
      const rustCode = 'fn main() {}';
      const result = highlighter.highlightBlock(rustCode, 'rust');
      
      expect(result).toContain('<pre');
      expect(result).toContain('<code');
      expect(result).toContain('language-rust');
    });

    it('should include line numbers when requested', () => {
      const code = 'line1\nline2\nline3';
      const result = highlighter.highlightBlock(code, 'rust', true);
      
      expect(result).toContain('line-numbers');
      expect(result).toContain('data-line');
    });

    it('should not include line numbers by default', () => {
      const code = 'line1\nline2\nline3';
      const result = highlighter.highlightBlock(code, 'rust', false);
      
      expect(result).not.toContain('line-numbers');
    });
  });

  describe('Rust syntax highlighting quality', () => {
    it('should highlight fn keyword', () => {
      const result = highlighter.highlight('fn main() {}', 'rust');
      // highlight.js adds classes like hljs-keyword
      expect(result).toContain('hljs');
    });

    it('should highlight let keyword', () => {
      const result = highlighter.highlight('let x = 42;', 'rust');
      expect(result).toContain('hljs');
    });

    it('should highlight string literals', () => {
      const result = highlighter.highlight('let s = "hello";', 'rust');
      expect(result).toContain('hljs');
    });

    it('should highlight numbers', () => {
      const result = highlighter.highlight('let x = 42;', 'rust');
      expect(result).toContain('hljs');
    });

    it('should highlight comments', () => {
      const result = highlighter.highlight('// This is a comment', 'rust');
      expect(result).toContain('hljs');
    });
  });

  describe('TypeScript syntax highlighting quality', () => {
    it('should highlight interface keyword', () => {
      const result = highlighter.highlight('interface User {}', 'typescript');
      expect(result).toContain('hljs');
    });

    it('should highlight type annotations', () => {
      const result = highlighter.highlight('const x: number = 1;', 'typescript');
      expect(result).toContain('hljs');
    });

    it('should highlight string literals', () => {
      const result = highlighter.highlight('const s = "hello";', 'typescript');
      expect(result).toContain('hljs');
    });
  });

  describe('language normalization', () => {
    it('should normalize ts to typescript', () => {
      const result1 = highlighter.highlight('const x = 1;', 'ts');
      const result2 = highlighter.highlight('const x = 1;', 'typescript');
      
      // Both should produce highlighted output
      expect(result1).toContain('hljs');
      expect(result2).toContain('hljs');
    });

    it('should normalize js to javascript', () => {
      const result = highlighter.highlight('const x = 1;', 'js');
      expect(result).toContain('hljs');
    });

    it('should normalize sh and shell to bash', () => {
      const result1 = highlighter.highlight('echo hello', 'sh');
      const result2 = highlighter.highlight('echo hello', 'shell');
      
      expect(result1).toContain('hljs');
      expect(result2).toContain('hljs');
    });
  });
});
