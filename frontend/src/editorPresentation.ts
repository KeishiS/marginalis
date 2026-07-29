import { ApiError, NoteDiagnostic, Problem } from "./api";
import { utf8ByteOffsetToLineColumn } from "./textPosition";

export function toProblem(error: unknown): Problem {
  if (error instanceof ApiError) {
    return error.problem;
  }
  return {
    code: "network_error",
    message: "通信に失敗しました。入力内容を保ったまま再試行できます。",
  };
}

export function problemMessage(problem: Problem): string {
  switch (problem.code) {
    case "validation_failed":
      return "入力内容を確認してください。";
    case "conflict":
      return "ほかの操作でノートが更新されました。";
    case "authentication_required":
      return "ログインの有効期限が切れました。再度ログインしてください。";
    default:
      return problem.message;
  }
}

export function diagnosticLocation(
  source: string,
  diagnostic: NoteDiagnostic,
): string {
  if (!canSelectDiagnostic(diagnostic)) {
    return "";
  }
  const location = utf8ByteOffsetToLineColumn(
    source,
    diagnostic.span?.start ?? 0,
  );
  return `${location.line}行${location.column}列: `;
}

export function canSelectDiagnostic(diagnostic: NoteDiagnostic): boolean {
  return (
    diagnostic.target.field === "source" &&
    diagnostic.span?.unit === "utf8_byte"
  );
}

export function editorStatus({
  saving,
  isDirty,
  failed,
  conflicted,
  notice,
}: {
  saving: boolean;
  isDirty: boolean;
  failed: boolean;
  conflicted: boolean;
  notice: string;
}): string {
  if (saving) return "保存しています…";
  if (conflicted) return "更新内容の競合を解消してください。";
  if (failed) return "保存に失敗しました。入力内容は維持されています。";
  if (notice) return notice;
  return isDirty ? "未保存の変更があります。" : "変更は保存されています。";
}

export function diagnosticMessage(code: string): string {
  switch (code) {
    case "invalid_title":
      return "題名を入力し、改行と上限を超える文字を取り除いてください。";
    case "invalid_tag":
      return "タグの空欄、改行、重複、または長さを確認してください。";
    case "too_many_tags":
      return "タグの数が上限を超えています。";
    case "source_too_large":
      return "AsciiDoc文書のデータ量が上限を超えています。";
    case "asciidoc_parse_failed":
      return "AsciiDoc本文を解析できませんでした。";
    case "include_directive_disabled":
      return "includeディレクティブは使用できません。";
    case "inline_passthrough_disabled":
    case "block_passthrough_disabled":
      return "未検証の内容を直接出力する記法は使用できません。";
    case "duplicate_anchor":
      return "同じアンカーが複数あります。";
    case "external_reference_disabled":
      return "外部の参照先は使用できません。";
    case "invalid_note_reference":
      return "ノート参照には正しいノートIDを指定してください。";
    case "invalid_url_scheme":
      return "許可されていない形式のURLです。";
    case "resource_disabled":
      return "外部リソースは使用できません。";
    case "unsupported_math_language":
      return "対応していない数式形式です。";
    case "unsupported_source_language":
      return "対応していないソースコード言語です。";
    case "trailing-whitespace":
      return "行末の不要な空白を取り除いてください。";
    case "excessive-blank-lines":
      return "連続する空行を減らしてください。";
    case "heading-marker-space":
      return "見出し記号の後に空白を入れてください。";
    case "asciidoc-file-link":
      return "AsciiDoc文書への参照にはxrefを使用してください。";
    case "non-asciidoc-xref":
      return "AsciiDoc以外の参照先には通常のリンクを使用してください。";
    case "macro-boundary":
      return "インラインマクロの前に空白を入れてください。";
    default:
      return "入力内容を確認してください。";
  }
}

export function diagnosticSeverityLabel(
  severity: NoteDiagnostic["severity"],
): string {
  switch (severity) {
    case "error":
      return "エラー";
    case "warning":
      return "警告";
    case "information":
      return "情報";
    case "hint":
      return "ヒント";
  }
}
