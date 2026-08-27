import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { isRegisteredCommand, parseCommand } from "./commands.js";
import { isAllowedChat } from "./authorization.js";

describe("Telegram command authorization boundary", () => {
  test("parses only registered slash commands", () => {
    assert.deepEqual(parseCommand("/status now"), {
      command: "/status",
      args: "now",
    });
    assert.equal(isRegisteredCommand("/status"), true);
    assert.equal(isRegisteredCommand("/status; rm -rf /"), false);
    assert.equal(isRegisteredCommand("status"), false);
  });

  test("allows configured chats and rejects unauthorized ids", () => {
    assert.equal(isAllowedChat(42, [42]), true);
    assert.equal(isAllowedChat(7, [42]), false);
    assert.equal(isAllowedChat(Number.NaN, [42]), false);
  });
});
