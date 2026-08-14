import { describe, expect, it } from "vitest";

import {
  NOTE_LIST_PAGE_SIZE,
  noteListSearch,
  parseNoteListQuery,
  selectNoteListPage,
} from "../src/noteListState";
import { NoteListEntry } from "../src/api";

function note(index: number, tags: string[] = ["research"]): NoteListEntry {
  return {
    note_id: `0197c9bc-0000-7000-8000-${String(index).padStart(12, "0")}`,
    title: `ノート${index}`,
    tags,
    updated_at_ms: Date.parse("2026-07-28T12:00:00Z") - index,
    revision: 1,
    created_via: "web",
    review_status: "pending",
    reviewed_revision: null,
    reviewed_at_ms: null,
    access: index % 2 === 0 ? "edit" : "manage",
  };
}

describe("note list state", () => {
  it("URLから重複のないタグ、日付、ページを読み取る", () => {
    expect(
      parseNoteListQuery(
        "?tag=research%2C+rust&tag=research&updated_after=2026-07-01&page=2",
      ),
    ).toEqual({
      tags: ["research", "rust"],
      updatedAfter: "2026-07-01",
      reviewStatus: "",
      page: 2,
    });
  });

  it("不正な日付とページを既定値へ戻す", () => {
    expect(
      parseNoteListQuery("?updated_after=2026-02-30&page=-1&review_status=x"),
    ).toEqual({
      tags: [],
      updatedAfter: "",
      reviewStatus: "",
      page: 1,
    });
  });

  it("全タグと更新日で絞り込み、ページ範囲を補正する", () => {
    const notes = [
      note(1, ["research", "rust"]),
      note(2, ["research"]),
      {
        ...note(3, ["research", "rust"]),
        updated_at_ms: Date.parse("2026-06-01T00:00:00Z"),
      },
    ];
    expect(
      selectNoteListPage(notes, {
        tags: ["research", "rust"],
        updatedAfter: "2026-07-01",
        reviewStatus: "",
        page: 9,
      }),
    ).toMatchObject({ notes: [notes[0]], page: 1, pageCount: 1, total: 1 });
  });

  it("更新日の境界を利用者のローカル暦日の開始として扱う", () => {
    const cutoff = new Date(2026, 2, 8).getTime();
    const before = { ...note(1), updated_at_ms: cutoff - 1 };
    const boundary = { ...note(2), updated_at_ms: cutoff };

    const page = selectNoteListPage([before, boundary], {
      tags: [],
      updatedAfter: "2026-03-08",
      reviewStatus: "",
      page: 1,
    });

    expect(page.notes).toEqual([boundary]);
  });

  it("夏時間の切替日にも指定地域の暦日境界を使う", () => {
    const originalTimeZone = process.env.TZ;
    process.env.TZ = "America/New_York";
    try {
      const before = {
        ...note(1),
        updated_at_ms: Date.parse("2026-03-08T04:59:59.999Z"),
      };
      const boundary = {
        ...note(2),
        updated_at_ms: Date.parse("2026-03-08T05:00:00Z"),
      };
      expect(
        selectNoteListPage([before, boundary], {
          tags: [],
          updatedAfter: "2026-03-08",
          reviewStatus: "",
          page: 1,
        }).notes,
      ).toEqual([boundary]);
    } finally {
      process.env.TZ = originalTimeZone;
    }
  });

  it("西暦100年より前の日付も1900年代へ読み替えない", () => {
    const boundary = new Date(0);
    boundary.setFullYear(99, 0, 2);
    boundary.setHours(0, 0, 0, 0);
    const included = { ...note(1), updated_at_ms: boundary.getTime() };
    const excluded = { ...note(2), updated_at_ms: boundary.getTime() - 1 };
    expect(
      selectNoteListPage([excluded, included], {
        tags: [],
        updatedAfter: "0099-01-02",
        reviewStatus: "",
        page: 1,
      }).notes,
    ).toEqual([included]);
  });

  it("固定件数でページ分割し、URLを正規化する", () => {
    const page = selectNoteListPage(
      Array.from({ length: NOTE_LIST_PAGE_SIZE + 1 }, (_, index) =>
        note(index + 1),
      ),
      { tags: [], updatedAfter: "", reviewStatus: "", page: 2 },
    );
    expect(page.notes).toHaveLength(1);
    expect(
      noteListSearch({
        tags: ["rust"],
        updatedAfter: "",
        reviewStatus: "",
        page: 2,
      }),
    ).toBe("?tag=rust&page=2");
  });

  it("確認状態で絞り込み、条件をURLへ反映する", () => {
    const pending = note(1);
    const reviewed = { ...note(2), review_status: "reviewed" as const };
    expect(
      selectNoteListPage([pending, reviewed], {
        tags: [],
        updatedAfter: "",
        reviewStatus: "reviewed",
        page: 1,
      }).notes,
    ).toEqual([reviewed]);
    expect(parseNoteListQuery("?review_status=pending").reviewStatus).toBe(
      "pending",
    );
    expect(
      noteListSearch({
        tags: [],
        updatedAfter: "",
        reviewStatus: "pending",
        page: 1,
      }),
    ).toBe("?review_status=pending");
  });
});
