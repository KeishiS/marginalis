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
  ).toContain("16 KiB以下");
});
