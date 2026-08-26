/**
 * Per-sender FIFO message queue.
 * Ensures only one message is processed per sender at a time.
 * Includes message ID deduplication with LRU eviction.
 */

export interface QueueEntry {
  sender: string;
  text: string;
  receivedAt: number;
}

const MSG_ID_CACHE_SIZE = 1000;

export class MessageQueue {
  private queues = new Map<string, QueueEntry[]>();
  private processing = new Set<string>();
  private seenMsgIds = new Map<string, number>(); // msgId → timestamp

  /**
   * Check if a message ID has been seen before (deduplication).
   * Returns true if the message is a duplicate.
   */
  isDuplicate(msgId: string): boolean {
    if (this.seenMsgIds.has(msgId)) return true;
    this.seenMsgIds.set(msgId, Date.now());
    // LRU eviction: remove oldest entries when cache exceeds limit
    if (this.seenMsgIds.size > MSG_ID_CACHE_SIZE) {
      const entries = Array.from(this.seenMsgIds.entries());
      entries.sort((a, b) => a[1] - b[1]);
      for (let i = 0; i < 100 && i < entries.length; i++) {
        this.seenMsgIds.delete(entries[i][0]);
      }
    }
    return false;
  }

  /**
   * Enqueue a message for a sender.
   * Returns false if the queue has reached maxQueueSize.
   */
  enqueue(
    sender: string,
    message: string,
    maxQueueSize: number = 100,
  ): boolean {
    if (this.queueLength(sender) >= maxQueueSize) return false;
    const entry: QueueEntry = { sender, text: message, receivedAt: Date.now() };
    const queue = this.queues.get(sender);
    if (queue) {
      queue.push(entry);
    } else {
      this.queues.set(sender, [entry]);
    }
    return true;
  }

  /**
   * Dequeue the next message for a sender (FIFO).
   * Returns null if the queue is empty.
   */
  dequeue(sender: string): QueueEntry | null {
    const queue = this.queues.get(sender);
    if (!queue || queue.length === 0) return null;
    return queue.shift()!;
  }

  /**
   * Check if a sender currently has a message being processed.
   */
  isProcessing(sender: string): boolean {
    return this.processing.has(sender);
  }

  /**
   * Mark a sender as currently processing.
   */
  startProcessing(sender: string): void {
    this.processing.add(sender);
  }

  /**
   * Mark a sender as done processing.
   */
  finishProcessing(sender: string): void {
    this.processing.delete(sender);
  }

  /**
   * Check if a sender has queued messages.
   */
  hasQueued(sender: string): boolean {
    const queue = this.queues.get(sender);
    return !!queue && queue.length > 0;
  }

  /**
   * Get the queue length for a sender.
   */
  queueLength(sender: string): number {
    return this.queues.get(sender)?.length ?? 0;
  }
}
