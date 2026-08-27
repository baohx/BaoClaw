import { strict as assert } from "node:assert";
import test from "node:test";
import { DEFAULTS, loadWhatsAppConfig, validateAllowFrom } from "./config.js";

test("missing WhatsApp config uses an empty allowlist", () => {
  const config = loadWhatsAppConfig(
    "/tmp/baoclaw-test-config-does-not-exist.json",
  );
  assert.deepEqual(config.allowFrom, DEFAULTS.allowFrom);
});

test("allowlist validation keeps only E.164 numbers", () => {
  const config = {
    ...DEFAULTS,
    allowFrom: ["+905551112233", "905551112233", "not-a-number"],
  };
  assert.deepEqual(validateAllowFrom(config), ["+905551112233"]);
});
