/** Split Telegram messages without exceeding the platform limit. */
export function splitMessage(text: string, max = 4096): string[] {
  if (max < 1) throw new RangeError("max must be greater than zero");
  if (text.length <= max) return [text];

  const chunks: string[] = [];
  let offset = 0;
  while (text.length - offset > max) {
    let end = offset + max;
    if (isHighSurrogate(text.charCodeAt(end - 1))) {
      end = end === offset + 1 ? end + 1 : end - 1;
    }
    let splitAt = text.lastIndexOf("\n\n", end);
    if (splitAt <= offset) splitAt = text.lastIndexOf("\n", end);
    if (splitAt <= offset) splitAt = end;
    chunks.push(text.slice(offset, splitAt));
    offset = splitAt;
  }
  if (offset < text.length) chunks.push(text.slice(offset));
  return chunks;
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff;
}
