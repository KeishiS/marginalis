import { describe, expect, it } from "vitest";

import {
  parseApplicationConfig,
  parseDeletedNoteListEntries,
  parseNote,
  parseNotePreview,
  parseNoteView,
  parseProblem,
} from "../src/generated/contracts";

describe("生成済みREST応答検査", () => {
  it("必須項目の型が異なるノートを拒否する", () => {
    expect(() =>
      parseNote({
        note_id: "0197c9bc-0000-7000-8000-000000000001",
        title: "題名",
        source: "= 題名\n\n本文",
        tags: [],
        created_at_ms: 1,
        updated_at_ms: 1,
        revision: "1",
      }),
    ).toThrow();
  });

  it("未知のproblem codeを拒否する", () => {
    expect(() =>
      parseProblem({ code: "unexpected", message: "unexpected" }),
    ).toThrow();
  });

  it("削除済み一覧の復元に必要な項目を検査する", () => {
    const value = {
      note_id: "0197c9bc-0000-7000-8000-000000000001",
      title: "削除済み",
      deleted_at_ms: 1,
      purge_at_ms: 2,
      revision: 3,
    };
    expect(parseDeletedNoteListEntries([value])).toEqual([value]);
    expect(() =>
      parseDeletedNoteListEntries([{ ...value, purge_at_ms: "tomorrow" }]),
    ).toThrow();
  });

  it("閲覧snapshotの関連概要まで検査する", () => {
    expect(() =>
      parseNoteView({
        note: {
          note_id: "0197c9bc-0000-7000-8000-000000000001",
          title: "題名",
          source: "= 題名\n\n本文",
          tags: [],
          created_at_ms: 1,
          updated_at_ms: 1,
          revision: 1,
        },
        access: "read",
        html: "<article></article>",
        math_macros: [],
        related: { outgoing: [{ title: "IDなし" }], incoming: [] },
      }),
    ).toThrow();
  });

  it("成功したプレビューの診断形式を検査する", () => {
    const preview = {
      html: "<p>本文</p>",
      math_macros: [],
      diagnostics: [
        {
          code: "macro-boundary",
          severity: "warning",
          target: { field: "source" },
          span: { start: 10, end: 14, unit: "utf8_byte" },
          message: "warning",
        },
      ],
    };
    expect(parseNotePreview(preview).diagnostics).toHaveLength(1);
    expect(() =>
      parseNotePreview({
        ...preview,
        diagnostics: [{ ...preview.diagnostics[0], severity: "unknown" }],
      }),
    ).toThrow();
    expect(() =>
      parseNotePreview({
        ...preview,
        diagnostics: [
          { ...preview.diagnostics[0], target: { field: "unknown" } },
        ],
      }),
    ).toThrow();
    expect(() =>
      parseNotePreview({
        ...preview,
        diagnostics: [
          {
            ...preview.diagnostics[0],
            span: { start: 10, end: 14, unit: "byte" },
          },
        ],
      }),
    ).toThrow();
  });
});

describe("起動設定の検査", () => {
  const config = {
    apiBase: "/api/v3",
    basePath: "/",
    path: "/",
    search: "",
    styleNonce: "nonce-value",
  };

  it("サーバーが埋め込む設定を読み取る", () => {
    expect(parseApplicationConfig(config)).toEqual(config);
  });

  it("項目が欠けた設定を拒否する", () => {
    const incomplete: Record<string, unknown> = { ...config };
    delete incomplete.styleNonce;
    expect(() => parseApplicationConfig(incomplete)).toThrow();
  });

  it("型が異なる設定を拒否する", () => {
    expect(() => parseApplicationConfig({ ...config, basePath: 1 })).toThrow();
  });

  it("設定そのものが無い場合を拒否する", () => {
    expect(() => parseApplicationConfig(null)).toThrow();
  });
});
