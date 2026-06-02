/**
 * Allowlist filter for WhatsApp Gateway.
 * Validates phone numbers against E.164 format and checks allowlist membership.
 */

const E164_REGEX = /^\+\d{7,15}$/;

/**
 * Validate that a phone number matches E.164 format: + followed by 7-15 digits.
 */
export function validateE164(phone: string): boolean {
  return E164_REGEX.test(phone);
}

/**
 * Extract the phone number from a WhatsApp JID.
 * JID formats this handles:
 *   "12025551234@s.whatsapp.net"     → "+12025551234"
 *   "12025551234:5@s.whatsapp.net"   → "+12025551234"  (drops device id)
 *   "12025551234:5@lid"              → "+12025551234"  (drops device id)
 *   "+12025551234"                   → "+12025551234"  (already normalized)
 * Senders coming in as `<lid-id>@lid` without an underlying phone-number
 * mapping are returned unchanged, since we have no phone digits to recover;
 * the caller may use Baileys' lid-mapping API to resolve the real number
 * before invoking this helper.
 */
export function normalizeJid(jid: string): string {
  if (jid.startsWith('+')) return jid;
  const atIdx = jid.indexOf('@');
  let digits = atIdx >= 0 ? jid.slice(0, atIdx) : jid;
  // Strip Baileys device-id suffix, e.g. "8613671505207:5" -> "8613671505207"
  const colonIdx = digits.indexOf(':');
  if (colonIdx >= 0) digits = digits.slice(0, colonIdx);
  // If the underlying id is not numeric (e.g. an opaque LID), preserve it
  // so the allowlist check fails closed instead of false-matching a phone.
  if (!/^\d+$/.test(digits)) {
    return atIdx >= 0 ? jid.slice(0, atIdx) : jid;
  }
  return '+' + digits;
}

/**
 * Check if a sender (JID or phone) is on the allowlist.
 * Returns true only if the normalized sender exactly matches an allowlist entry.
 * Empty allowlist rejects all senders.
 */
export function isAllowed(sender: string, allowlist: string[]): boolean {
  if (allowlist.length === 0) return false;
  const normalized = normalizeJid(sender);
  return allowlist.includes(normalized);
}
