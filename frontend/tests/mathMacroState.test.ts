import { expect, test } from "vitest";

import {
  mathMacroBytes,
  MAX_MATH_MACRO_TOTAL_BYTES,
  validateMathMacros,
} from "../src/mathMacroState";
import validationCases from "./fixtures/math-macro-validation.json";

test("Rustと共有する境界例を同じ結果に判定する", () => {
  for (const validationCase of validationCases) {
    const actual =
      validateMathMacros([
        {
          name: validationCase.name,
          replacement: validationCase.replacement,
          argument_count: validationCase.argument_count,
        },
      ]) === null;
    expect(actual, validationCase.description).toBe(validationCase.valid);
  }
});

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
