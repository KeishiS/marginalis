export function utf8ByteOffsetToLineColumn(
  value: string,
  byteOffset: number,
): { line: number; column: number } {
  const encoder = new TextEncoder();
  let consumed = 0;
  let line = 1;
  let column = 1;
  let previousWasCarriageReturn = false;
  for (const character of value) {
    const byteLength = encoder.encode(character).length;
    if (consumed + byteLength > byteOffset) {
      break;
    }
    consumed += byteLength;
    if (character === "\r") {
      line += 1;
      column = 1;
      previousWasCarriageReturn = true;
    } else if (character === "\n") {
      if (!previousWasCarriageReturn) {
        line += 1;
      }
      column = 1;
      previousWasCarriageReturn = false;
    } else {
      column += 1;
      previousWasCarriageReturn = false;
    }
  }
  return { line, column };
}

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
