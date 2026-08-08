import type { MathMacro } from "./api";

// 公開契約とapplication層の入力規則に対応する。contracts.test.tsで生成schemaとの一致を固定する。
export const MAX_MATH_MACROS = 64;
// JSON Schemaの一項目には表せない、配列全体のUTF-8バイト数に対するapplication規則。
export const MAX_MATH_MACRO_TOTAL_BYTES = 16 * 1024;
export const MAX_MATH_MACRO_NAME_CHARACTERS = 32;
export const MAX_MATH_MACRO_REPLACEMENT_CHARACTERS = 512;
export const MAX_MATH_MACRO_ARGUMENTS = 9;

export function mathMacroBytes(macros: MathMacro[]): number {
  const encoder = new TextEncoder();
  return macros.reduce(
    (total, macro) =>
      total +
      encoder.encode(macro.name).length +
      encoder.encode(macro.replacement).length,
    0,
  );
}

export function validateMathMacros(macros: MathMacro[]): string | null {
  if (macros.length > MAX_MATH_MACROS) {
    return `数式マクロは${MAX_MATH_MACROS}件まで設定できます。`;
  }
  if (mathMacroBytes(macros) > MAX_MATH_MACRO_TOTAL_BYTES) {
    return "コマンド名と置換内容の合計を16 KiB以下にしてください。";
  }
  const names = new Set<string>();
  for (const macro of macros) {
    if (
      !/^[A-Za-z]+$/.test(macro.name) ||
      Array.from(macro.name).length > MAX_MATH_MACRO_NAME_CHARACTERS
    ) {
      return "コマンド名は32文字以内の半角英字で入力してください。";
    }
    if (names.has(macro.name)) {
      return `コマンド名「${macro.name}」が重複しています。`;
    }
    names.add(macro.name);
    if (
      macro.replacement.length === 0 ||
      Array.from(macro.replacement).length >
        MAX_MATH_MACRO_REPLACEMENT_CHARACTERS ||
      Array.from(macro.replacement).some((character) =>
        /\p{Cc}/u.test(character),
      )
    ) {
      return "置換内容は制御文字を含めず、1文字以上512文字以内で入力してください。";
    }
    if (
      !Number.isInteger(macro.argument_count) ||
      macro.argument_count < 0 ||
      macro.argument_count > MAX_MATH_MACRO_ARGUMENTS
    ) {
      return "引数の数は0から9までの整数で入力してください。";
    }
    const invalidReference = Array.from(
      macro.replacement.matchAll(/#([^1-9]|$)/g),
    ).length;
    const references = Array.from(macro.replacement.matchAll(/#([1-9])/g));
    if (
      invalidReference > 0 ||
      references.some((match) => Number(match[1]) > macro.argument_count)
    ) {
      return `コマンド「${macro.name}」の置換内容に、引数の数を超える参照があります。`;
    }
  }
  return null;
}
