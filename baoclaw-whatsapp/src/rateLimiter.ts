/**
 * Sliding window rate limiter per sender.
 * Default: 20 messages per 60-second window.
 */

export class RateLimiter {
  private windows = new Map<string, number[]>();

  constructor(
    private maxMessages: number = 20,
    private windowMs: number = 60_000,
  ) {}

  /**
   * Try to consume a rate limit token for the given sender.
   * Returns true if the message is allowed, false if rate-limited.
   */
  tryConsume(sender: string, now: number = Date.now()): boolean {
    const cutoff = now - this.windowMs;
    let timestamps = this.windows.get(sender);
    if (!timestamps) {
      timestamps = [];
      this.windows.set(sender, timestamps);
    }
    // Remove expired timestamps
    while (timestamps.length > 0 && timestamps[0] <= cutoff) {
      timestamps.shift();
    }
    if (timestamps.length >= this.maxMessages) {
      return false;
    }
    timestamps.push(now);
    return true;
  }

  /**
   * Get remaining quota for a sender in the current window.
   */
  getRemainingQuota(sender: string, now: number = Date.now()): number {
    const cutoff = now - this.windowMs;
    const timestamps = this.windows.get(sender);
    if (!timestamps) return this.maxMessages;
    const active = timestamps.filter(t => t > cutoff);
    return Math.max(0, this.maxMessages - active.length);
  }
}
