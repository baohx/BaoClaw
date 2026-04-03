/**
 * Bug Condition Exploration Test — ESM Import Extension Fix
 *
 * This test verifies Property 1 (Bug Condition): every relative import
 * specifier in the affected ts-ipc/ files ends with `.js`.
 *
 * On UNFIXED code this test is EXPECTED TO FAIL, confirming the bug exists.
 *
 * Validates: Requirements 1.1, 1.2, 2.1, 2.2
 */
import { describe, it } from 'node:test';
import * as assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';

/** Files known to contain buggy relative imports */
const AFFECTED_FILES = [
  'cli.ts',
  'index.ts',
  'rustCore.ts',
  'useRustEngine.ts',
  'streamHandler.ts',
];

/**
 * Extract all import/export specifiers from a TypeScript source string.
 * Matches patterns like:
 *   import ... from 'specifier'
 *   export ... from 'specifier'
 */
function extractImportSpecifiers(source: string): string[] {
  const regex = /(?:import|export)\s+.*?\s+from\s+['"]([^'"]+)['"]/g;
  const specifiers: string[] = [];
  let match: RegExpExecArray | null;
  while ((match = regex.exec(source)) !== null) {
    specifiers.push(match[1]);
  }
  return specifiers;
}

/**
 * Bug condition from design doc:
 *   isBugCondition(specifier, file) =
 *     file IS IN affected set
 *     AND specifier STARTS WITH './'
 *     AND NOT specifier ENDS WITH '.js'
 */
function isBugCondition(specifier: string): boolean {
  return specifier.startsWith('./') && !specifier.endsWith('.js');
}

describe('Property 1: Bug Condition — All relative imports include .js extension', () => {
  const dir = path.dirname(new URL(import.meta.url).pathname);

  for (const file of AFFECTED_FILES) {
    it(`${file}: every relative import specifier ends with .js`, () => {
      const filePath = path.join(dir, file);
      const source = fs.readFileSync(filePath, 'utf-8');
      const specifiers = extractImportSpecifiers(source);

      // There must be at least one relative specifier in each affected file
      const relativeSpecifiers = specifiers.filter((s) => s.startsWith('./'));
      assert.ok(
        relativeSpecifiers.length > 0,
        `Expected at least one relative import in ${file}, found none`,
      );

      // Assert every relative specifier ends with .js
      for (const specifier of relativeSpecifiers) {
        assert.ok(
          !isBugCondition(specifier),
          `${file}: relative import '${specifier}' is missing .js extension`,
        );
      }
    });
  }
});

/**
 * Preservation Property Test — Bare Specifier Imports Unchanged
 *
 * Property 2: For all files in ts-ipc/, for all import specifiers that do NOT
 * start with './' or '../' (bare specifiers), assert the specifier has no .js
 * extension appended.
 *
 * This captures the full set of bare specifiers from UNFIXED code and verifies
 * they remain correct (no spurious .js extensions).
 *
 * Validates: Requirements 3.1, 3.2, 3.3
 */

/** All .ts source files in ts-ipc/ (excluding test files) */
const ALL_SOURCE_FILES = [
  'cli.ts',
  'client.ts',
  'index.ts',
  'markdownRenderer.ts',
  'rustCore.ts',
  'streamHandler.ts',
  'types.ts',
  'useRustEngine.ts',
];

/**
 * Known bare specifiers per file — snapshot from UNFIXED code.
 * These are Node.js built-in and third-party package imports that must
 * never have .js appended.
 */
const KNOWN_BARE_SPECIFIERS: Record<string, string[]> = {
  'cli.ts': ['net', 'readline', 'path', 'child_process', 'fs', 'os'],
  'client.ts': ['net'],
  'index.ts': [],
  'markdownRenderer.ts': [],
  'rustCore.ts': ['child_process'],
  'streamHandler.ts': [],
  'types.ts': [],
  'useRustEngine.ts': ['react'],
};

/**
 * Determine if a specifier is a bare specifier (not relative).
 * Bare specifiers do NOT start with './' or '../'.
 */
function isBareSpecifier(specifier: string): boolean {
  return !specifier.startsWith('./') && !specifier.startsWith('../');
}

describe('Property 2: Preservation — Bare specifier imports unchanged', () => {
  const dir = path.dirname(new URL(import.meta.url).pathname);

  for (const file of ALL_SOURCE_FILES) {
    it(`${file}: no bare specifier has .js extension appended`, () => {
      const filePath = path.join(dir, file);
      const source = fs.readFileSync(filePath, 'utf-8');
      const specifiers = extractImportSpecifiers(source);
      const bareSpecifiers = specifiers.filter(isBareSpecifier);

      // Assert no bare specifier ends with .js
      for (const specifier of bareSpecifiers) {
        assert.ok(
          !specifier.endsWith('.js'),
          `${file}: bare specifier '${specifier}' should NOT have .js extension`,
        );
      }
    });

    it(`${file}: bare specifiers match known snapshot`, () => {
      const filePath = path.join(dir, file);
      const source = fs.readFileSync(filePath, 'utf-8');
      const specifiers = extractImportSpecifiers(source);
      const bareSpecifiers = specifiers.filter(isBareSpecifier);
      const expected = KNOWN_BARE_SPECIFIERS[file] ?? [];

      assert.deepStrictEqual(
        bareSpecifiers.sort(),
        [...expected].sort(),
        `${file}: bare specifiers do not match expected snapshot. ` +
        `Found: [${bareSpecifiers.join(', ')}], Expected: [${expected.join(', ')}]`,
      );
    });
  }
});
