import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { splitMessage } from "./messageSplitter.js";

describe("splitMessage", () => {
  test("returns short messages unchanged", () => {
    assert.deepStrictEqual(splitMessage("hello", 4096), ["hello"]);
  });

  test("splits at paragraph boundaries when possible", () => {
    const text = "a".repeat(8) + "\n\n" + "b".repeat(8);
    assert.deepStrictEqual(splitMessage(text, 10), [
      "a".repeat(8),
      "\n\n" + "b".repeat(8),
    ]);
  });

  test("splits long words at the limit", () => {
    const chunks = splitMessage("x".repeat(4097), 4096);
    assert.deepStrictEqual(
      chunks.map((chunk) => chunk.length),
      [4096, 1],
    );
  });

  test("accepts an exact-limit message", () => {
    assert.deepStrictEqual(splitMessage("x".repeat(4096)), ["x".repeat(4096)]);
  });

  test("does not split an emoji surrogate pair", () => {
    const text = "a".repeat(4095) + "😀";
    const chunks = splitMessage(text, 4096);
    assert.deepStrictEqual(chunks, ["a".repeat(4095), "😀"]);
    assert.ok(chunks.every((chunk) => !chunk.includes("\ufffd")));
  });

  test("rejects a non-positive limit", () => {
    assert.throws(() => splitMessage("hello", 0), RangeError);
  });

  test("does not loop when the limit is smaller than an emoji", () => {
    assert.deepStrictEqual(splitMessage("😀", 1), ["😀"]);
  });
});
