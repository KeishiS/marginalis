export function utf8ByteOffsetToTextOffset(
  value: string,
  byteOffset: number,
): number {
  const encoder = new TextEncoder();
  let consumedBytes = 0;
  let consumedCodeUnits = 0;
  for (const character of value) {
    const byteLength = encoder.encode(character).length;
    if (consumedBytes + byteLength > byteOffset) {
      break;
    }
    consumedBytes += byteLength;
    consumedCodeUnits += character.length;
  }
  return consumedCodeUnits;
}
