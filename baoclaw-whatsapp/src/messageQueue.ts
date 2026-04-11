/**
 * Per-sender FIFO message queue.
 * Ensures only one message is processed per sender at a time.
 */

export interface QueueEntry {
  sender: string;
  text: string;
  receivedAt: number;
}

export class MessageQueue {
  private queues = new Map<string, QueueEntry[]>();
  private processing = new Set<string>();

  /**
   * Enqueue a message for a sender.
   */
  enqueue(sender: string, message: string): void {
    const entry: QueueEntry = { sender, text: message, receivedAt: Date.now() };
    const queue = this.queues.get(sender);
    if (queue) {
      queue.push(entry);
    } else {
      this.queues.set(sender, [entry]);
    }
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
