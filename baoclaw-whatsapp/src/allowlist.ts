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
 * JID format: "12025551234@s.whatsapp.net" → "+12025551234"
 * Also handles already-normalized "+12025551234" format.
 */
export function normalizeJid(jid: string): string {
  if (jid.startsWith('+')) return jid;
  const atIdx = jid.indexOf('@');
  const digits = atIdx >= 0 ? jid.slice(0, atIdx) : jid;
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
