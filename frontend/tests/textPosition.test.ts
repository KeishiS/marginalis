import { describe, expect, it } from "vitest";

import {
  utf8ByteOffsetToTextOffset,
  utf8ByteOffsetsToTextOffsets,
} from "../src/textPosition";

describe("UTF-8バイトオフセットの一括変換", () => {
  it("昇順の列を1件ずつの変換と同じ結果へ直す", () => {
    const source = "= 題名\n\n**太字**と😀絵文字\r\nascii text\n";
    const encoder = new TextEncoder();
    const total = encoder.encode(source).length;
    const offsets = Array.from({ length: total + 1 }, (_, index) => index);
    expect(utf8ByteOffsetsToTextOffsets(source, offsets)).toEqual(
      offsets.map((offset) => utf8ByteOffsetToTextOffset(source, offset)),
    );
  });

  it("絵文字はUTF-16で2単位として数える", () => {
    // "😀"はUTF-8で4バイト、UTF-16で2単位。
    const source = "a😀b";
    expect(utf8ByteOffsetsToTextOffsets(source, [0, 1, 5])).toEqual([0, 1, 3]);
  });

  it("本文の長さを超えるオフセットは末尾へ丸める", () => {
    const source = "短い";
    expect(utf8ByteOffsetsToTextOffsets(source, [100, 200])).toEqual([2, 2]);
  });

  it("空の列と空の本文を受け付ける", () => {
    expect(utf8ByteOffsetsToTextOffsets("abc", [])).toEqual([]);
    expect(utf8ByteOffsetsToTextOffsets("", [0, 3])).toEqual([0, 0]);
  });
});
