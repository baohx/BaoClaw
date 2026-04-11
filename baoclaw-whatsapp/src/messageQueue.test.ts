import { test, describe } from 'node:test';
import assert from 'node:assert';
import { MessageQueue } from './messageQueue.js';

// ── Unit Tests ──

describe('MessageQueue', () => {
  test('enqueue and dequeue in FIFO order', () => {
    const mq = new MessageQueue();
    mq.enqueue('+1111', 'first');
    mq.enqueue('+1111', 'second');
    mq.enqueue('+1111', 'third');

    assert.strictEqual(mq.dequeue('+1111')?.text, 'first');
    assert.strictEqual(mq.dequeue('+1111')?.text, 'second');
    assert.strictEqual(mq.dequeue('+1111')?.text, 'third');
    assert.strictEqual(mq.dequeue('+1111'), null);
  });

  test('tracks processing state', () => {
    const mq = new MessageQueue();
    assert.strictEqual(mq.isProcessing('+1111'), false);
    mq.startProcessing('+1111');
    assert.strictEqual(mq.isProcessing('+1111'), true);
    mq.finishProcessing('+1111');
    assert.strictEqual(mq.isProcessing('+1111'), false);
  });

  test('hasQueued returns correct state', () => {
    const mq = new MessageQueue();
    assert.strictEqual(mq.hasQueued('+1111'), false);
    mq.enqueue('+1111', 'msg');
    assert.strictEqual(mq.hasQueued('+1111'), true);
    mq.dequeue('+1111');
    assert.strictEqual(mq.hasQueued('+1111'), false);
  });

  test('queueLength returns correct count', () => {
    const mq = new MessageQueue();
    assert.strictEqual(mq.queueLength('+1111'), 0);
    mq.enqueue('+1111', 'a');
    mq.enqueue('+1111', 'b');
    assert.strictEqual(mq.queueLength('+1111'), 2);
  });

  test('senders are independent', () => {
    const mq = new MessageQueue();
    mq.enqueue('+1111', 'for-1');
    mq.enqueue('+2222', 'for-2');
    assert.strictEqual(mq.dequeue('+1111')?.text, 'for-1');
    assert.strictEqual(mq.dequeue('+2222')?.text, 'for-2');
    assert.strictEqual(mq.dequeue('+1111'), null);
  });
});
