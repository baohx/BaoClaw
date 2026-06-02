/**
 * SenderTracker — per-sender state management for the WhatsApp Gateway.
 *
 * Replaces the hardcoded JID mapping and `responseAccumulators` Map in gateway.ts
 * with a unified per-sender state object keyed by phone number.
 *
 * Each sender (identified by normalized E.164 phone) tracks:
 *   - reply-target JID (direct or group)
 *   - whether the conversation is in a group chat
 *   - accumulated assistant response text
 *   - pending permission request (if any)
 *   - total messages processed
 */

// ─── Types ────────────────────────────────────────────────────────────────────

/** An active permission request awaiting user approval. */
export interface PermissionRequest {
  /** Unique ID from the daemon's tool_use event. */
  tool_use_id: string;
  /** Name of the tool that requires permission. */
  tool_name: string;
  /** Human-readable description of what the tool will do. */
  description: string;
  /** Unix timestamp (ms) when this request expires. */
  expiresAt: number;
}

/** Full state kept for every registered sender. */
export interface SenderState {
  /** The WhatsApp JID to send replies to (user@… or group@…). */
  jid: string;
  /** `true` when the JID is a group chat (`@g.us`). */
  isGroup: boolean;
  /** Incrementally built response text from assistant_chunk events. */
  responseAccumulator: string;
  /** Non-null when the daemon paused for user approval of a tool call. */
  pendingPermission: PermissionRequest | null;
  /** Running count of messages processed for this sender. */
  messageCount: number;
}

// ─── Implementation ───────────────────────────────────────────────────────────

/**
 * Centralised tracker for all active WhatsApp senders.
 *
 * Design notes:
 *  - Keyed by **phone number** (E.164, normalised by `allowlist.normalizeJid`).
 *  - Registering an existing phone preserves its accumulator & pending permission
 *    so that reconnects / duplicate messages don't lose in-flight state.
 *  - All public methods are synchronous — this class holds no I/O.
 */
export class SenderTracker {
  /** Phone → complete sender state. */
  private senders = new Map<string, SenderState>();

  // ── Registration ──────────────────────────────────────────────────────────

  /**
   * Register a sender or update an existing entry.
   *
   * - **New sender**: creates a fresh `SenderState` with an empty accumulator
   *   and `messageCount = 1`.
   * - **Existing sender**: updates `jid` / `isGroup` (the sender may have
   *   moved between DM and group), preserves `responseAccumulator` and
   *   `pendingPermission`, and increments `messageCount`.
   *
   * @param phone  Normalised E.164 phone number (e.g. `"8613800138000"`).
   * @param jid    WhatsApp JID to reply to.
   * @param isGroup  `true` if the JID is a group chat.
   */
  registerSender(phone: string, jid: string, isGroup: boolean): void {
    const existing = this.senders.get(phone);
    if (existing) {
      existing.jid = jid;
      existing.isGroup = isGroup;
      existing.messageCount += 1;
    } else {
      this.senders.set(phone, {
        jid,
        isGroup,
        responseAccumulator: '',
        pendingPermission: null,
        messageCount: 1,
      });
    }
  }

  // ── Lookups ───────────────────────────────────────────────────────────────

  /**
   * Return the reply-target JID for `phone`, or `null` if the sender
   * has never been registered.
   */
  getJid(phone: string): string | null {
    return this.senders.get(phone)?.jid ?? null;
  }

  /**
   * Return the complete `SenderState` for `phone`, or `undefined` if
   * the sender is not registered.
   */
  getState(phone: string): SenderState | undefined {
    return this.senders.get(phone);
  }

  /**
   * Check whether `phone` has been registered.
   */
  hasSender(phone: string): boolean {
    return this.senders.has(phone);
  }

  // ── Response accumulator ──────────────────────────────────────────────────

  /**
   * Append `content` to the sender's response accumulator.
   *
   * If the sender is not registered the call is silently ignored.
   */
  accumulate(phone: string, content: string): void {
    const state = this.senders.get(phone);
    if (state) {
      state.responseAccumulator += content;
    }
  }

  /**
   * Return the accumulated response text for `phone`.
   *
   * Returns `''` if the sender is not registered.
   */
  getAccumulated(phone: string): string {
    return this.senders.get(phone)?.responseAccumulator ?? '';
  }

  /**
   * Clear the response accumulator without removing the sender entry.
   *
   * Typically called after the full response has been sent to WhatsApp.
   * If the sender is not registered the call is silently ignored.
   */
  clearAccumulator(phone: string): void {
    const state = this.senders.get(phone);
    if (state) {
      state.responseAccumulator = '';
    }
  }

  // ── Permission requests ──────────────────────────────────────────────────

  /**
   * Set the pending permission request for `phone`.
   *
   * **Important**: if a previous request was already pending it is **replaced**
   * (i.e. implicitly denied). The caller is responsible for handling the
   * denial — e.g. `PermissionManager.registerRequest` calls `onTimeout` for
   * the old request before invoking this method.
   *
   * If the sender is not registered the call is silently ignored.
   */
  setPendingPermission(phone: string, request: PermissionRequest): void {
    const state = this.senders.get(phone);
    if (state) {
      state.pendingPermission = request;
    }
  }

  /**
   * Return the current pending permission request, or `null` if none.
   */
  getPendingPermission(phone: string): PermissionRequest | null {
    return this.senders.get(phone)?.pendingPermission ?? null;
  }

  /**
   * Clear (remove) the pending permission request for `phone`.
   *
   * Does **not** delete the sender. If the sender is not registered the call
   * is silently ignored.
   */
  clearPendingPermission(phone: string): void {
    const state = this.senders.get(phone);
    if (state) {
      state.pendingPermission = null;
    }
  }

  // ── Statistics ────────────────────────────────────────────────────────────

  /**
   * Count senders that currently have a non-empty accumulator **or** a
   * pending permission request — i.e. senders in the middle of an active
   * interaction with the daemon.
   */
  getActiveSenderCount(): number {
    let count = 0;
    const states = Array.from(this.senders.values());
    for (const state of states) {
      if (state.responseAccumulator.length > 0 || state.pendingPermission !== null) {
        count += 1;
      }
    }
    return count;
  }

  /**
   * Return an array of all registered phone numbers.
   */
  getAllSenders(): string[] {
    return Array.from(this.senders.keys());
  }
}
