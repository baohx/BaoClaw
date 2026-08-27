import { test } from "node:test";
import assert from "node:assert/strict";
import { redactSensitiveText } from "./logger.js";

test("redacts credentials from diagnostic messages", () => {
  const result = redactSensitiveText(
    "Bearer abc123 api_key=sk-test-secret password: hunter2",
  );
  assert.equal(result.includes("abc123"), false);
  assert.equal(result.includes("sk-test-secret"), false);
  assert.equal(result.includes("hunter2"), false);
});
