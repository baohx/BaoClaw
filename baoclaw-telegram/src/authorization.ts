/** Pure inbound authorization check kept independent from Telegram startup. */
export function isAllowedChat(
  chatId: number,
  allowedChatIds: readonly number[],
): boolean {
  return Number.isSafeInteger(chatId) && allowedChatIds.includes(chatId);
}
