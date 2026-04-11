import { test, describe } from 'node:test';
import assert from 'node:assert';
import fc from 'fast-check';
import { isAllowed, validateE164, normalizeJid } from './allowlist.js';

// ── Unit Tests ──

describe('validateE164', () => {
  test('accepts valid E.164 numbers', () => {
    assert.strictEqual(validateE164('+12025551234'), true);
    assert.strictEqual(validateE164('+447911123456'), true);
    assert.strictEqual(validateE164('+1234567'), true);       // 7 digits (minimum)
    assert.strictEqual(validateE164('+123456789012345'), true); // 15 digits (maximum)
  });

  test('rejects invalid strings', () => {
    assert.strictEqual(validateE164('1234567890'), false);     // no +
    assert.strictEqual(validateE164('+1'), false);             // too short (1 digit)
    assert.strictEqual(validateE164('+123456'), false);        // 6 digits, below minimum
    assert.strictEqual(validateE164('+1234567890123456'), false); // 16 digits, above max
    assert.strictEqual(validateE164('+12a45678'), false);      // contains letter
    assert.strictEqual(validateE164(''), false);
    assert.strictEqual(validateE164('+'), false);
    assert.strictEqual(validateE164('hello'), false);
    assert.strictEqual(validateE164('+12 345 6789'), false);   // spaces
  });
});

describe('normalizeJid', () => {
  test('extracts phone from WhatsApp JID', () => {
    assert.strictEqual(normalizeJid('12025551234@s.whatsapp.net'), '+12025551234');
  });

  test('passes through already-normalized numbers', () => {
    assert.strictEqual(normalizeJid('+12025551234'), '+12025551234');
  });

  test('handles JID without @', () => {
    assert.strictEqual(normalizeJid('12025551234'), '+12025551234');
  });
});

describe('isAllowed', () => {
  test('returns true for allowlisted number', () => {
    assert.strictEqual(isAllowed('+12025551234', ['+12025551234', '+447911123456']), true);
  });

  test('returns false for non-allowlisted number', () => {
    assert.strictEqual(isAllowed('+19999999999', ['+12025551234']), false);
  });

  test('returns false for empty allowlist', () => {
    assert.strictEqual(isAllowed('+12025551234', []), false);
  });

  test('normalizes JID before checking', () => {
    assert.strictEqual(isAllowed('12025551234@s.whatsapp.net', ['+12025551234']), true);
  });
});


// ── Property-Based Tests ──

// Feature: whatsapp-gateway, Property 7: Allowlist membership check
describe('PBT: Property 7 — Allowlist membership check', () => {
  /**
   * Validates: Requirements 8.2, 8.5
   * For any phone number and any list of E.164 phone numbers,
   * isAllowed returns true iff the sender (after JID normalization) exactly matches an entry.
   * Empty allowlist rejects all senders.
   */
  test('isAllowed returns true iff sender in list, empty list rejects all', () => {
    const e164Arb = fc.tuple(
      fc.integer({ min: 1, max: 999 }),
      fc.integer({ min: 1000000, max: 999999999999 }),
    ).map(([cc, num]) => `+${cc}${num}`).filter(p => /^\+\d{7,15}$/.test(p));

    fc.assert(
      fc.property(
        e164Arb,
        fc.array(e164Arb, { minLength: 0, maxLength: 10 }),
        (sender, allowlist) => {
          const result = isAllowed(sender, allowlist);
          if (allowlist.length === 0) {
            assert.strictEqual(result, false, 'empty allowlist should reject all');
          } else if (allowlist.includes(sender)) {
            assert.strictEqual(result, true, `sender ${sender} should be allowed when in list`);
          } else {
            assert.strictEqual(result, false, `sender ${sender} should be rejected when not in list`);
          }
        },
      ),
      { numRuns: 200 },
    );
  });
});

// Feature: whatsapp-gateway, Property 8: E.164 phone number validation
describe('PBT: Property 8 — E.164 validation', () => {
  /**
   * Validates: Requirements 8.3
   * validateE164 returns true iff the string matches /^\+\d{7,15}$/
   */
  test('validateE164 returns true iff /^\\+\\d{7,15}$/', () => {
    const E164_REGEX = /^\+\d{7,15}$/;

    // Test with arbitrary strings
    fc.assert(
      fc.property(
        fc.string({ minLength: 0, maxLength: 20 }),
        (s) => {
          assert.strictEqual(validateE164(s), E164_REGEX.test(s));
        },
      ),
      { numRuns: 200 },
    );

    // Test with strings that look like phone numbers (+ prefix + digits)
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 20 }).chain(len =>
          fc.stringOf(fc.constantFrom('0','1','2','3','4','5','6','7','8','9'), { minLength: len, maxLength: len })
        ),
        (digits) => {
          const phone = '+' + digits;
          assert.strictEqual(validateE164(phone), E164_REGEX.test(phone));
        },
      ),
      { numRuns: 200 },
    );
  });
});
