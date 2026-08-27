/** Pure inbound authorization checks kept independent from gateway startup. */
export function isAllowedChat(
  chatId: string,
  allowedChatIds: readonly string[],
): boolean {
  return (
    typeof chatId === "string" &&
    chatId.trim().length > 0 &&
    !/[\u0000-\u001f\u007f]/.test(chatId) &&
    allowedChatIds.includes(chatId)
  );
}
