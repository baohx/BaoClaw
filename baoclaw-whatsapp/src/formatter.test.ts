import { test, describe } from 'node:test';
import assert from 'node:assert';
import fc from 'fast-check';
import { formatForWhatsApp, splitMessage, formatToolUse, formatError } from './formatter.js';

// ── Unit Tests ──

describe('formatForWhatsApp', () => {
  test('converts **bold** to *bold*', () => {
    assert.strictEqual(formatForWhatsApp('**hello**'), '*hello*');
  });

  test('converts *italic* to _italic_', () => {
    assert.strictEqual(formatForWhatsApp('*hello*'), '_hello_');
  });

  test('preserves _italic_ as-is', () => {
    assert.strictEqual(formatForWhatsApp('_hello_'), '_hello_');
  });

  test('preserves code blocks', () => {
    const input = '```js\nconsole.log("hi");\n```';
    assert.strictEqual(formatForWhatsApp(input), input);
  });

  test('preserves inline code', () => {
    assert.strictEqual(formatForWhatsApp('use `npm install`'), 'use `npm install`');
  });

  test('handles mixed formatting', () => {
    assert.strictEqual(formatForWhatsApp('**bold** and *italic*'), '*bold* and _italic_');
  });
});

describe('splitMessage', () => {
  test('returns single chunk for short messages', () => {
    const result = splitMessage('hello', 4096);
    assert.deepStrictEqual(result, ['hello']);
  });

  test('splits long messages', () => {
    const long = 'a'.repeat(5000);
    const chunks = splitMessage(long, 4096);
    assert.ok(chunks.length >= 2);
    for (const chunk of chunks) {
      assert.ok(chunk.length <= 4096);
    }
  });

  test('concatenation preserves original', () => {
    const text = 'paragraph one\n\nparagraph two\n\n' + 'x'.repeat(4000);
    const chunks = splitMessage(text, 4096);
    assert.strictEqual(chunks.join(''), text);
  });
});

describe('formatToolUse', () => {
  test('prefixes with ⚡', () => {
    assert.strictEqual(formatToolUse('Bash'), '⚡ Bash');
  });
});

describe('formatError', () => {
  test('prefixes with ❌', () => {
    assert.strictEqual(formatError('RPC_ERROR', 'timeout'), '❌ [RPC_ERROR] timeout');
  });
});


// ── Property-Based Tests ──

// Feature: whatsapp-gateway, Property 5: Message splitting preserves content
describe('PBT: Property 5 — Message splitting preserves content', () => {
  /**
   * Validates: Requirements 7.4
   * All chunks ≤ maxLength, concatenation equals original.
   */
  test('all chunks ≤ maxLength and concatenation equals original', () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 12000 }),
        fc.integer({ min: 10, max: 4096 }),
        (text, maxLength) => {
          const chunks = splitMessage(text, maxLength);

          // Every chunk must be ≤ maxLength
          for (let i = 0; i < chunks.length; i++) {
            assert.ok(
              chunks[i].length <= maxLength,
              `chunk ${i} length ${chunks[i].length} exceeds maxLength ${maxLength}`,
            );
          }

          // Concatenation must equal original
          assert.strictEqual(chunks.join(''), text);
        },
      ),
      { numRuns: 200 },
    );
  });
});

// Feature: whatsapp-gateway, Property 3: Markdown-to-WhatsApp formatting conversion
describe('PBT: Property 3 — Markdown formatting', () => {
  /**
   * Validates: Requirements 7.1, 7.2, 7.3
   * **bold** → *bold*, *italic* → _italic_, code blocks preserved.
   */
  test('**bold** converts to *bold*', () => {
    // Generate words without *, _, or ` to avoid ambiguity
    const safeWord = fc.stringOf(
      fc.constantFrom(...'abcdefghijklmnopqrstuvwxyz0123456789 '.split('')),
      { minLength: 1, maxLength: 20 },
    ).filter(s => s.trim().length > 0);

    fc.assert(
      fc.property(safeWord, (word) => {
        const input = `**${word}**`;
        const output = formatForWhatsApp(input);
        assert.strictEqual(output, `*${word}*`);
      }),
      { numRuns: 200 },
    );
  });

  test('*italic* converts to _italic_', () => {
    const safeWord = fc.stringOf(
      fc.constantFrom(...'abcdefghijklmnopqrstuvwxyz0123456789 '.split('')),
      { minLength: 1, maxLength: 20 },
    ).filter(s => s.trim().length > 0);

    fc.assert(
      fc.property(safeWord, (word) => {
        const input = `*${word}*`;
        const output = formatForWhatsApp(input);
        assert.strictEqual(output, `_${word}_`);
      }),
      { numRuns: 200 },
    );
  });

  test('code blocks are preserved', () => {
    const safeCode = fc.stringOf(
      fc.constantFrom(...'abcdefghijklmnopqrstuvwxyz0123456789 \n=;(){}[]'.split('')),
      { minLength: 0, maxLength: 50 },
    );

    fc.assert(
      fc.property(safeCode, (code) => {
        const input = '```\n' + code + '\n```';
        const output = formatForWhatsApp(input);
        assert.strictEqual(output, input);
      }),
      { numRuns: 200 },
    );
  });
});
