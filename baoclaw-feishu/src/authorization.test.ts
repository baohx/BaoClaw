import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { isAllowedChat } from "./authorization.js";

describe("Feishu inbound authorization", () => {
  test("allows an exact configured chat id", () => {
    assert.equal(isAllowedChat("chat-a", ["chat-a"]), true);
  });

  test("rejects unknown, empty, and injection-like chat ids", () => {
    const allowed = ["chat-a"];
    assert.equal(isAllowedChat("chat-b", allowed), false);
    assert.equal(isAllowedChat("", allowed), false);
    assert.equal(isAllowedChat("chat-a\nchat-b", allowed), false);
    assert.equal(isAllowedChat("   ", ["   "]), false);
  });
});
