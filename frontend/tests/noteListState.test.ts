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
      page: 2,
    });
  });

  it("不正な日付とページを既定値へ戻す", () => {
    expect(parseNoteListQuery("?updated_after=2026-02-30&page=-1")).toEqual({
      tags: [],
      updatedAfter: "",
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
        page: 9,
      }),
    ).toMatchObject({ notes: [notes[0]], page: 1, pageCount: 1, total: 1 });
  });

  it("固定件数でページ分割し、URLを正規化する", () => {
    const page = selectNoteListPage(
      Array.from({ length: NOTE_LIST_PAGE_SIZE + 1 }, (_, index) =>
        note(index + 1),
      ),
      { tags: [], updatedAfter: "", page: 2 },
    );
    expect(page.notes).toHaveLength(1);
    expect(noteListSearch({ tags: ["rust"], updatedAfter: "", page: 2 })).toBe(
      "?tag=rust&page=2",
    );
  });
});
