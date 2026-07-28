import { describe, expect, it } from "vitest";

import {
  accessControlReducer,
  initialAccessControlState,
} from "../src/accessControlState";
import { editorReducer, initialEditorState } from "../src/editorState";

const note = {
  note_id: "0197c9bc-0000-7000-8000-000000000001",
  title: "保存済み",
  source: "= 保存済み\n:tags: 設計\n\n本文",
  tags: ["設計"],
  created_at_ms: 1,
  updated_at_ms: 2,
  revision: 2,
};

describe("editorReducer", () => {
  it("保存応答を編集内容と比較基準へ原子的に反映する", () => {
    const changed = editorReducer(initialEditorState(""), {
      type: "change",
      field: "source",
      value: "= 編集中\n",
    });
    const saved = editorReducer(changed, { type: "accept-note", note });
    expect(saved.form).toEqual(saved.baseline);
    expect(saved.noteId).toBe(note.note_id);
    expect(saved.revision).toBe(2);
  });

  it("競合解消では編集中の内容を保って比較基準だけを更新する", () => {
    const loaded = editorReducer(initialEditorState(note.note_id), {
      type: "accept-note",
      note,
    });
    const editing = editorReducer(loaded, {
      type: "change",
      field: "source",
      value: "= 保存済み\n\n編集中の本文",
    });
    const current = { ...note, source: "= 保存済み\n\n他の更新", revision: 3 };
    const rebased = editorReducer(
      editorReducer(editing, { type: "conflict", current }),
      { type: "rebase", note: current },
    );
    expect(rebased.form.source).toContain("編集中の本文");
    expect(rebased.baseline.source).toContain("他の更新");
    expect(rebased.revision).toBe(3);
  });
});

describe("accessControlReducer", () => {
  it("ACL取得時のETag revisionを保存の基準にする", () => {
    const loaded = accessControlReducer(initialAccessControlState(1), {
      type: "loaded",
      entries: [],
      revision: 4,
    });
    expect(loaded.revision).toBe(4);
  });

  it("同じsubjectの権限を重複させず置換する", () => {
    let state = accessControlReducer(initialAccessControlState(1), {
      type: "loaded",
      entries: [{ subject: "reader", permission: "read" }],
      revision: 1,
    });
    state = accessControlReducer(state, { type: "subject", value: "reader" });
    state = accessControlReducer(state, {
      type: "permission",
      value: "edit",
    });
    state = accessControlReducer(state, { type: "add" });
    expect(state.entries).toEqual([{ subject: "reader", permission: "edit" }]);
  });
});
