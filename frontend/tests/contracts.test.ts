import { describe, expect, it } from "vitest";

import {
  parseNote,
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
        related: { outgoing: [{ title: "IDなし" }], incoming: [] },
      }),
    ).toThrow();
  });
});
