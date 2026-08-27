import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { formatForFeishu } from "./formatter.js";

describe("formatForFeishu", () => {
  test("converts supported HTML formatting", () => {
    assert.strictEqual(
      formatForFeishu("<strong>bold</strong><br><em>italic</em>"),
      "**bold**\n*italic*",
    );
  });

  test("converts HTML tables to an ASCII table", () => {
    const result = formatForFeishu(
      "<table><tr><th>Name</th><th>Count</th></tr><tr><td>jobs</td><td>2</td></tr></table>",
    );
    assert.match(result, /\| Name \| Count \|/);
    assert.match(result, /\| jobs \| 2\s+\|/);
  });

  test("strips unsupported tags while preserving content", () => {
    assert.strictEqual(
      formatForFeishu("<script>alert(1)</script><p>Hello</p>"),
      "alert(1)\nHello",
    );
  });

  test("decodes HTML entities and preserves code blocks", () => {
    const code = "```js\nconst value = '<b>&amp;</b>';\n```";
    assert.strictEqual(
      formatForFeishu(`${code}\n&amp;`),
      "```js\nconst value = '<b>&</b>';\n```\n&",
    );
  });

  test("limits output to Feishu's message size", () => {
    assert.strictEqual(formatForFeishu("x".repeat(15001)).length, 15000);
  });
});
