/**
 * PermissionManager — state machine for tool-use permission requests.
 *
 * When the daemon needs user approval before executing a tool (e.g. file writes,
 * shell commands), this module:
 *   1. Formats a human-readable permission request message for WhatsApp.
 *   2. Registers the request in `SenderTracker` with a 60-second auto-expiry.
 *   3. Parses the user's WhatsApp reply (`yes`/`no`) and forwards the decision
 *      back to the daemon via `IpcClient.request('permissionResponse', …)`.
 *
 * Lifecycle of a single permission request:
 *   registerRequest()  →  [waiting for user]  →  handleResponse("yes"/"no")
 *                                                or 60 s timeout → onTimeout()
 */

import { SenderTracker, type PermissionRequest } from "./senderTracker.js";
import { IpcClient } from "../../ts-ipc/index.js";
import { createLogger } from "../../ts-ipc/logger.js";

const logger = createLogger("whatsapp");

/** Time (ms) before an unanswered permission request is automatically denied. */
const PERMISSION_TIMEOUT_MS = 60_000; // 60 seconds

/**
 * Manages permission request / response flow on behalf of the WhatsApp Gateway.
 *
 * Usage:
 * ```ts
 * const pm = new PermissionManager(senderTracker);
 * // When a tool_use event arrives:
 * const text = pm.formatPermissionRequest(toolUseId, toolName, desc);
 * await sock.sendMessage(jid, { text });
 * pm.registerRequest(phone, toolUseId, toolName, desc, (ph, id) => { … });
 *
 * // When an inbound WhatsApp message arrives:
 * const handled = await pm.handleResponse(phone, msgText, ipcClient);
 * if (handled) { /* was a permission reply, already forwarded to daemon *\/ }
 *
 * // On shutdown:
 * pm.cleanup();
 * ```
 */
export class PermissionManager {
  private senderTracker: SenderTracker;
  /** Per-phone timeout handles so we can cancel them on explicit replies. */
  private timers = new Map<string, ReturnType<typeof setTimeout>>();

  /**
   * @param senderTracker  The shared `SenderTracker` instance that stores
   *                       per-sender state including `pendingPermission`.
   */
  constructor(senderTracker: SenderTracker) {
    this.senderTracker = senderTracker;
  }

  // ── Formatting ────────────────────────────────────────────────────────────

  /**
   * Build a formatted permission request message suitable for WhatsApp.
   *
   * The returned string uses WhatsApp-friendly formatting (bold with `*…*`).
   *
   * @param toolUseId    Opaque ID from the daemon's `tool_use` event.
   * @param toolName     Human-readable tool name (e.g. `"bash"`).
   * @param description  Optional one-liner describing what the tool will do.
   * @returns A multi-line string ready to be sent via `sock.sendMessage`.
   *
   * @example
   * ```ts
   * const text = pm.formatPermissionRequest('tu_123', 'bash', 'rm -rf /tmp/old');
   * // 🔐 *权限请求*
   * // 工具: bash
   * // 描述: rm -rf /tmp/old
   * //
   * // 请回复 *yes* 允许 或 *no* 拒绝
   * // （60秒后自动拒绝）
   * ```
   */
  formatPermissionRequest(
    toolUseId: string,
    toolName: string,
    description?: string,
  ): string {
    const desc = description?.trim() || "无";
    return [
      "🔐 *权限请求*",
      `工具: ${toolName}`,
      `描述: ${desc}`,
      "",
      "请回复 *yes* 允许 或 *no* 拒绝",
      "（60秒后自动拒绝）",
    ].join("\n");
  }

  // ── Registration ──────────────────────────────────────────────────────────

  /**
   * Register a new permission request for `phone`.
   *
   * Steps:
   *  1. If the sender already has a **pending** request, cancel its timer and
   *     invoke `onTimeout(phone, oldToolUseId)` so the caller can deny the
   *     stale request with the daemon.
   *  2. Create a `PermissionRequest` object with an expiry timestamp
   *     (`Date.now() + 60_000`).
   *  3. Store it via `SenderTracker.setPendingPermission`.
   *  4. Start a 60-second timer that, on expiry, clears the pending permission
   *     and invokes `onTimeout(phone, toolUseId)`.
   *
   * @param phone       Sender phone (E.164).
   * @param toolUseId   Unique ID from the daemon.
   * @param toolName    Tool name for the request record.
   * @param description Human-readable description.
   * @param onTimeout   Callback invoked when the request expires without a
   *                    reply **or** when superseded by a newer request.
   */
  registerRequest(
    phone: string,
    toolUseId: string,
    toolName: string,
    description: string,
    onTimeout: (phone: string, toolUseId: string) => void,
  ): void {
    // 1. Evict any existing pending request for this sender.
    const existing = this.senderTracker.getPendingPermission(phone);
    if (existing) {
      const oldTimer = this.timers.get(phone);
      if (oldTimer !== undefined) {
        clearTimeout(oldTimer);
        this.timers.delete(phone);
      }
      // Notify caller about the superseded request so it can deny it.
      onTimeout(phone, existing.tool_use_id);
    }

    // 2. Build the new request.
    const request: PermissionRequest = {
      tool_use_id: toolUseId,
      tool_name: toolName,
      description,
      expiresAt: Date.now() + PERMISSION_TIMEOUT_MS,
    };

    // 3. Persist in the tracker.
    this.senderTracker.setPendingPermission(phone, request);

    // 4. Start the auto-expiry timer.
    const timer = setTimeout(() => {
      this.senderTracker.clearPendingPermission(phone);
      this.timers.delete(phone);
      onTimeout(phone, toolUseId);
    }, PERMISSION_TIMEOUT_MS);

    // Prevent the timer from keeping the Node.js event loop alive during
    // a clean shutdown (cleanup() will handle it explicitly).
    timer.unref();

    this.timers.set(phone, timer);
  }

  // ── Response handling ─────────────────────────────────────────────────────

  /**
   * Process an inbound WhatsApp text message as a potential permission reply.
   *
   * The method is **idempotent** — if no permission is pending for the sender
   * it simply returns `false` and the caller can treat the message as a normal
   * chat prompt.
   *
   * Recognised keywords (case-insensitive, trimmed):
   *   - `"yes"`, `"allow"`  → allow
   *   - `"no"`, `"deny"`    → deny
   *
   * When a valid keyword is detected:
   *   1. The decision is forwarded to the daemon via
   *      `ipcClient.request('permissionResponse', { tool_use_id, decision })`.
   *   2. The pending permission and its timer are cleared.
   *   3. Returns `true`.
   *
   * If the text does **not** match any keyword but a permission **is** pending,
   * the method still returns `false` — the caller should handle the text as a
   * regular message (and may optionally warn the user).
   *
   * @param phone      Sender phone (E.164).
   * @param text       Raw message text from WhatsApp.
   * @param ipcClient  Connected IPC client for communicating with the daemon.
   * @returns `true` if this message was a permission reply (fully handled),
   *          `false` otherwise.
   */
  async handleResponse(
    phone: string,
    text: string,
    ipcClient: IpcClient,
  ): Promise<boolean> {
    // 1. Check for a pending request.
    const pending = this.senderTracker.getPendingPermission(phone);
    if (!pending) {
      return false;
    }

    // 2. Parse the reply.
    const normalized = text.trim().toLowerCase();
    let decision: "allow" | "deny" | null = null;

    if (normalized === "yes" || normalized === "allow") {
      decision = "allow";
    } else if (normalized === "no" || normalized === "deny") {
      decision = "deny";
    }

    // 3. Not a recognised keyword — leave the request pending.
    if (decision === null) {
      return false;
    }

    // 4. Forward the decision to the daemon.
    try {
      await ipcClient.request("permissionResponse", {
        tool_use_id: pending.tool_use_id,
        decision,
      });
    } catch (err) {
      // Swallow IPC errors — the daemon may have disconnected. We still
      // clean up the local state so the user is not stuck.
      logger.error(
        `Failed to send permissionResponse for ${pending.tool_use_id}: ${err}`,
      );
    }

    // 5. Clean up local state.
    this.senderTracker.clearPendingPermission(phone);
    const timer = this.timers.get(phone);
    if (timer !== undefined) {
      clearTimeout(timer);
      this.timers.delete(phone);
    }

    // 6. Signal that the message was handled.
    return true;
  }

  // ── Lifecycle ─────────────────────────────────────────────────────────────

  /**
   * Clear all pending timers.
   *
   * Must be called during gateway shutdown to prevent dangling `setTimeout`
   * handles. After calling this method no new timers should be created on
   * the same instance.
   */
  cleanup(): void {
    const allTimers = Array.from(this.timers.values());
    for (const timer of allTimers) {
      clearTimeout(timer);
    }
    this.timers.clear();
  }
}
