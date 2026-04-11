import { test, describe } from 'node:test';
import assert from 'node:assert';
import fc from 'fast-check';
import { RateLimiter } from './rateLimiter.js';

// ── Unit Tests ──

describe('RateLimiter', () => {
  test('accepts first 20 messages', () => {
    const rl = new RateLimiter(20, 60_000);
    const now = 1000000;
    for (let i = 0; i < 20; i++) {
      assert.strictEqual(rl.tryConsume('+12025551234', now + i), true);
    }
  });

  test('rejects 21st message within window', () => {
    const rl = new RateLimiter(20, 60_000);
    const now = 1000000;
    for (let i = 0; i < 20; i++) {
      rl.tryConsume('+12025551234', now + i);
    }
    assert.strictEqual(rl.tryConsume('+12025551234', now + 20), false);
  });

  test('resets after window expires', () => {
    const rl = new RateLimiter(20, 60_000);
    const now = 1000000;
    for (let i = 0; i < 20; i++) {
      rl.tryConsume('+12025551234', now);
    }
    assert.strictEqual(rl.tryConsume('+12025551234', now + 1000), false);
    // After 60s window
    assert.strictEqual(rl.tryConsume('+12025551234', now + 60_001), true);
  });

  test('tracks senders independently', () => {
    const rl = new RateLimiter(20, 60_000);
    const now = 1000000;
    for (let i = 0; i < 20; i++) {
      rl.tryConsume('+1111', now);
    }
    assert.strictEqual(rl.tryConsume('+1111', now + 1), false);
    assert.strictEqual(rl.tryConsume('+2222', now + 1), true);
  });

  test('getRemainingQuota returns correct values', () => {
    const rl = new RateLimiter(20, 60_000);
    const now = 1000000;
    assert.strictEqual(rl.getRemainingQuota('+1111', now), 20);
    for (let i = 0; i < 5; i++) {
      rl.tryConsume('+1111', now);
    }
    assert.strictEqual(rl.getRemainingQuota('+1111', now), 15);
  });
});


// ── Property-Based Tests ──

// Feature: whatsapp-gateway, Property 9: Rate limiter sliding window
describe('PBT: Property 9 — Rate limiter sliding window', () => {
  /**
   * Validates: Requirements 9.1, 9.4
   * First 20 messages accepted, 21st rejected within a 60s window.
   * After 60s from the first message, the counter resets.
   */
  test('first 20 accepted, 21st rejected in 60s window', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 100 }),  // number of messages to send
        fc.integer({ min: 0, max: 1_000_000 }),  // base timestamp
        (count, baseTime) => {
          const rl = new RateLimiter(20, 60_000);
          const sender = '+12025551234';

          // Send `count` messages all at the same timestamp (within window)
          const results: boolean[] = [];
          for (let i = 0; i < count; i++) {
            results.push(rl.tryConsume(sender, baseTime));
          }

          // First 20 should be accepted
          for (let i = 0; i < Math.min(count, 20); i++) {
            assert.strictEqual(results[i], true, `message ${i + 1} should be accepted`);
          }

          // Messages 21+ should be rejected
          for (let i = 20; i < count; i++) {
            assert.strictEqual(results[i], false, `message ${i + 1} should be rejected`);
          }
        },
      ),
      { numRuns: 200 },
    );
  });

  test('window reset allows new messages after expiry', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 1_000_000 }),  // base timestamp
        fc.integer({ min: 60_001, max: 200_000 }),  // time after window
        (baseTime, elapsed) => {
          const rl = new RateLimiter(20, 60_000);
          const sender = '+12025551234';

          // Fill the window
          for (let i = 0; i < 20; i++) {
            rl.tryConsume(sender, baseTime);
          }
          assert.strictEqual(rl.tryConsume(sender, baseTime + 100), false);

          // After window expires, should accept again
          assert.strictEqual(rl.tryConsume(sender, baseTime + elapsed), true);
        },
      ),
      { numRuns: 200 },
    );
  });
});
