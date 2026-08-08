import type { MathMacro } from "./api";
import { CONTRACT_SCHEMAS } from "./generated/contracts";

const settingsSchema = CONTRACT_SCHEMAS.MathMacroSettings as {
  properties: {
    macros: {
      maxItems: number;
      "x-marginalis-max-name-replacement-bytes": number;
    };
  };
};
const macroSchema = CONTRACT_SCHEMAS.MathMacro as {
  properties: {
    name: { maxLength: number };
    replacement: { maxLength: number };
    argument_count: { maximum: number };
  };
};

// 数値を画面へ写さず、application規則から生成した公開契約を正本として読む。
export const MAX_MATH_MACROS = settingsSchema.properties.macros.maxItems;
export const MAX_MATH_MACRO_TOTAL_BYTES =
  settingsSchema.properties.macros["x-marginalis-max-name-replacement-bytes"];
export const MAX_MATH_MACRO_NAME_CHARACTERS =
  macroSchema.properties.name.maxLength;
export const MAX_MATH_MACRO_REPLACEMENT_CHARACTERS =
  macroSchema.properties.replacement.maxLength;
export const MAX_MATH_MACRO_ARGUMENTS =
  macroSchema.properties.argument_count.maximum;

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
    return `コマンド名と置換内容の合計を${MAX_MATH_MACRO_TOTAL_BYTES}バイト以下にしてください。`;
  }
  const names = new Set<string>();
  for (const macro of macros) {
    const problem = validateMathMacro(macro);
    if (problem !== null) return problem;
    if (names.has(macro.name)) {
      return `コマンド名「${macro.name}」が重複しています。`;
    }
    names.add(macro.name);
  }
  return null;
}

/** 旧版で保存された値を含め、TeX定義へ安全に埋め込める一項目かを判定する。 */
export function validMathMacroForRendering(macro: MathMacro): boolean {
  return validateMathMacro(macro) === null;
}

function validateMathMacro(macro: MathMacro): string | null {
  if (
    !/^[A-Za-z]+$/.test(macro.name) ||
    Array.from(macro.name).length > MAX_MATH_MACRO_NAME_CHARACTERS
  ) {
    return `コマンド名は${MAX_MATH_MACRO_NAME_CHARACTERS}文字以内の半角英字で入力してください。`;
  }
  if (macro.name === "def") {
    return "コマンド名「def」は数式マクロの定義に使用するため指定できません。";
  }
  if (
    macro.replacement.length === 0 ||
    Array.from(macro.replacement).length >
      MAX_MATH_MACRO_REPLACEMENT_CHARACTERS ||
    Array.from(macro.replacement).some((character) => /\p{Cc}/u.test(character))
  ) {
    return `置換内容は制御文字を含めず、1文字以上${MAX_MATH_MACRO_REPLACEMENT_CHARACTERS}文字以内で入力してください。`;
  }
  if (!safeTexDefinitionReplacement(macro.replacement)) {
    return "置換内容の波括弧を対応させ、%は\\%と記述し、末尾を単独の\\にしないでください。";
  }
  if (
    !Number.isInteger(macro.argument_count) ||
    macro.argument_count < 0 ||
    macro.argument_count > MAX_MATH_MACRO_ARGUMENTS
  ) {
    return `引数の数は0から${MAX_MATH_MACRO_ARGUMENTS}までの整数で入力してください。`;
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
  return null;
}

function safeTexDefinitionReplacement(replacement: string): boolean {
  let braceDepth = 0;
  let consecutiveBackslashes = 0;
  for (const character of replacement) {
    if (character === "\\") {
      consecutiveBackslashes += 1;
      continue;
    }
    const escaped = consecutiveBackslashes % 2 === 1;
    consecutiveBackslashes = 0;
    if (character === "%" && !escaped) return false;
    if (character === "{" && !escaped) braceDepth += 1;
    if (character === "}" && !escaped) {
      if (braceDepth === 0) return false;
      braceDepth -= 1;
    }
  }
  return braceDepth === 0 && consecutiveBackslashes % 2 === 0;
}
