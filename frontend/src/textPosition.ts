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

/**
 * 昇順に並んだUTF-8バイトオフセットの列を、本文の1回の走査でUTF-16オフセットへ直す。
 *
 * span注釈のように多数のオフセットを変換する用途で、1件ごとに全文を数え直す
 * {@link utf8ByteOffsetToTextOffset}の繰り返しを避ける。文字の途中を指すオフセットは
 * その文字の先頭へ丸め、本文の長さを超えるオフセットは末尾へ丸める。
 */
export function utf8ByteOffsetsToTextOffsets(
  value: string,
  byteOffsets: readonly number[],
): number[] {
  const encoder = new TextEncoder();
  const results = new Array<number>(byteOffsets.length);
  let index = 0;
  let consumedBytes = 0;
  let consumedCodeUnits = 0;
  for (const character of value) {
    if (index >= byteOffsets.length) {
      break;
    }
    const byteLength = encoder.encode(character).length;
    while (
      index < byteOffsets.length &&
      byteOffsets[index] < consumedBytes + byteLength
    ) {
      results[index] = consumedCodeUnits;
      index += 1;
    }
    consumedBytes += byteLength;
    consumedCodeUnits += character.length;
  }
  while (index < byteOffsets.length) {
    results[index] = consumedCodeUnits;
    index += 1;
  }
  return results;
}
