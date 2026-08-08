import { expect, test } from "vitest";

import {
  mathMacroBytes,
  MAX_MATH_MACRO_TOTAL_BYTES,
  validateMathMacros,
} from "../src/mathMacroState";

test("マクロ名、置換内容、引数参照を保存規則と同じ条件で検査する", () => {
  expect(
    validateMathMacros([
      { name: "日本語", replacement: "x", argument_count: 0 },
    ]),
  ).toContain("半角英字");
  expect(
    validateMathMacros([
      { name: "valid", replacement: "x\ny", argument_count: 0 },
    ]),
  ).toContain("制御文字");
  expect(
    validateMathMacros([
      { name: "valid", replacement: "#2", argument_count: 1 },
    ]),
  ).toContain("引数の数を超える");
  expect(
    validateMathMacros([
      { name: "valid", replacement: "#1", argument_count: 1 },
    ]),
  ).toBeNull();
});

test("TeX定義を壊す名前、波括弧、comment、末尾backslashを拒否する", () => {
  for (const [name, replacement] of [
    ["def", "x"],
    ["broken", "{x"],
    ["broken", "}x"],
    ["broken", "x%comment"],
    ["broken", String.raw`x\\%comment`],
    ["broken", "x\\"],
  ]) {
    expect(
      validateMathMacros([{ name, replacement, argument_count: 0 }]),
      `name=${name}, replacement=${JSON.stringify(replacement)}`,
    ).not.toBeNull();
  }
  for (const replacement of [
    String.raw`{x}`,
    String.raw`\{x\}`,
    String.raw`x\%`,
    String.raw`x\\{y}\\`,
  ]) {
    expect(
      validateMathMacros([{ name: "safe", replacement, argument_count: 0 }]),
      `replacement=${JSON.stringify(replacement)}`,
    ).toBeNull();
  }
});

test("非BMP文字をUnicode code point単位で数える", () => {
  expect(
    validateMathMacros([
      { name: "emoji", replacement: "😀".repeat(300), argument_count: 0 },
    ]),
  ).toBeNull();
});

test("全体の大きさをUTF-8のバイト数で数える", () => {
  expect(
    mathMacroBytes([{ name: "a", replacement: "あ", argument_count: 0 }]),
  ).toBe(4);
  expect(
    validateMathMacros([
      {
        name: "large",
        replacement: "あ".repeat(
          Math.floor(MAX_MATH_MACRO_TOTAL_BYTES / 3) + 1,
        ),
        argument_count: 0,
      },
    ]),
  ).toContain(`${MAX_MATH_MACRO_TOTAL_BYTES}バイト以下`);
});
